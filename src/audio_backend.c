/* HALLS native playback adapters. PCM is synthesized by Rust, never read from files. */
#include <stdlib.h>
#include <stdint.h>
typedef void (*halls_callback)(void*, float*, unsigned);
#define FRAMES 480
#define CHANNELS 2

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mmsystem.h>
typedef struct {
    HWAVEOUT device; HANDLE wake, thread;
    volatile LONG stop, running;
    halls_callback render; void* user;
    WAVEHDR headers[3]; short samples[3][FRAMES*CHANNELS];
    float mix[FRAMES*CHANNELS]; unsigned prepared;
} Audio;
static DWORD WINAPI worker(void* opaque) {
    Audio* a=(Audio*)opaque;
    while (!InterlockedCompareExchange(&a->stop,0,0)) {
        ResetEvent(a->wake);
        for (unsigned i=0;i<3;i++) {
            WAVEHDR* h=&a->headers[i];
            if (h->dwFlags & WHDR_INQUEUE) continue;
            a->render(a->user,a->mix,FRAMES);
            for (unsigned j=0;j<FRAMES*CHANNELS;j++) {
                float sample=a->mix[j];
                if (sample>1) sample=1; if (sample<-1) sample=-1;
                a->samples[i][j]=(short)(sample*32767.0f);
            }
            if (waveOutWrite(a->device,h,sizeof(*h))!=MMSYSERR_NOERROR) goto done;
        }
        WaitForSingleObject(a->wake,50);
    }
done:
    InterlockedExchange(&a->running,0); return 0;
}
void halls_audio_close(void* opaque) {
    Audio* a=(Audio*)opaque; if (!a) return;
    InterlockedExchange(&a->stop,1);
    if(a->wake)SetEvent(a->wake);
    if(a->thread){WaitForSingleObject(a->thread,INFINITE);CloseHandle(a->thread);}
    if(a->device){
        waveOutReset(a->device);
        for(unsigned i=0;i<a->prepared;i++)waveOutUnprepareHeader(a->device,&a->headers[i],sizeof(WAVEHDR));
        waveOutClose(a->device);
    }
    if(a->wake)CloseHandle(a->wake); HeapFree(GetProcessHeap(),0,a);
}
void* halls_audio_open(halls_callback render,void* user) {
    Audio* a=(Audio*)HeapAlloc(GetProcessHeap(),HEAP_ZERO_MEMORY,sizeof(Audio)); if(!a)return NULL;
    a->render=render; a->user=user;
    a->wake=CreateEventW(NULL,TRUE,FALSE,NULL);
    WAVEFORMATEX format={WAVE_FORMAT_PCM,2,48000,192000,4,16,0};
    if(!a->wake)goto fail;
    if(waveOutOpen(&a->device,WAVE_MAPPER,&format,(DWORD_PTR)a->wake,0,CALLBACK_EVENT)!=MMSYSERR_NOERROR){a->device=NULL;goto fail;}
    for(unsigned i=0;i<3;i++) {
        a->headers[i].lpData=(LPSTR)a->samples[i];a->headers[i].dwBufferLength=sizeof(a->samples[i]);
        if(waveOutPrepareHeader(a->device,&a->headers[i],sizeof(WAVEHDR))!=MMSYSERR_NOERROR)goto fail;
        a->prepared++;
    }
    a->running=1;a->thread=CreateThread(NULL,0,worker,a,0,NULL);
    if(!a->thread)goto fail;
    return a;
fail: halls_audio_close(a);return NULL;
}
int halls_audio_running(void* opaque){Audio* a=(Audio*)opaque;return a && InterlockedCompareExchange(&a->running,0,0);}

#elif defined(__APPLE__)
#include <AudioToolbox/AudioToolbox.h>
#include <stdatomic.h>
typedef struct {AudioQueueRef queue; halls_callback render;void* user; atomic_int running;} Audio;
static void output(void* user,AudioQueueRef queue,AudioQueueBufferRef buffer) {
    Audio* a=(Audio*)user; if(!atomic_load(&a->running))return;
    a->render(a->user,(float*)buffer->mAudioData,FRAMES);
    buffer->mAudioDataByteSize=FRAMES*CHANNELS*sizeof(float);
    if(AudioQueueEnqueueBuffer(queue,buffer,0,NULL)!=noErr)atomic_store(&a->running,0);
}
void halls_audio_close(void* opaque) {
    Audio* a=(Audio*)opaque;if(!a)return;
    atomic_store(&a->running,0);
    if(a->queue){AudioQueueStop(a->queue,true);AudioQueueDispose(a->queue,true);}
    free(a);
}
void* halls_audio_open(halls_callback render,void* user) {
    Audio* a=(Audio*)calloc(1,sizeof(Audio));if(!a)return NULL;
    a->render=render;a->user=user;
    AudioStreamBasicDescription format={48000,kAudioFormatLinearPCM,kAudioFormatFlagIsFloat|kAudioFormatFlagIsPacked,8,1,8,2,32,0};
    if(AudioQueueNewOutput(&format,output,a,NULL,NULL,0,&a->queue)!=noErr)goto fail;
    atomic_store(&a->running,1);
    for(int i=0;i<3;i++) {
        AudioQueueBufferRef buffer=NULL;
        if(AudioQueueAllocateBuffer(a->queue,FRAMES*CHANNELS*sizeof(float),&buffer)!=noErr)goto fail;
        output(a,a->queue,buffer);
        if(!atomic_load(&a->running))goto fail;
    }
    if(AudioQueueStart(a->queue,NULL)!=noErr)goto fail;
    return a;
fail:halls_audio_close(a);return NULL;
}
int halls_audio_running(void* opaque){Audio* a=(Audio*)opaque;return a && atomic_load(&a->running);}

#else
/* Load the desktop's ALSA service library. No ALSA headers or vendored library. */
#include <dlfcn.h>
#include <pthread.h>
#include <stdatomic.h>
#include <time.h>
#include <errno.h>
typedef struct {
    void* library;void* pcm;pthread_t thread;int started;
    atomic_int stop,running;halls_callback render;void* user;
    int (*open)(void**,const char*,int,int);
    int (*params)(void*,int,int,unsigned,unsigned,int,unsigned);
    long (*write)(void*,const void*,unsigned long);
    int (*prepare)(void*);int (*drop)(void*);int (*close)(void*);
    float mix[FRAMES*CHANNELS];short samples[FRAMES*CHANNELS];
} Audio;
static void* worker(void* opaque) {
    Audio* a=(Audio*)opaque;
    while(!atomic_load(&a->stop)) {
        a->render(a->user,a->mix,FRAMES);
        for(unsigned i=0;i<FRAMES*CHANNELS;i++)a->samples[i]=(short)(a->mix[i]*32767.0f);
        unsigned offset=0;
        while(offset<FRAMES && !atomic_load(&a->stop)) {
            long n=a->write(a->pcm,a->samples+offset*CHANNELS,FRAMES-offset);
            if(n>0)offset+=(unsigned)n;
            else if(n==-EAGAIN || n==0){struct timespec delay={0,2000000};nanosleep(&delay,NULL);}
            else if(n==-EINTR)continue;
            else if((n!=-EPIPE && n!=-ESTRPIPE) || a->prepare(a->pcm)<0)goto done;
        }
    }
done:atomic_store(&a->running,0);return NULL;
}
void halls_audio_close(void* opaque) {
    Audio* a=(Audio*)opaque;if(!a)return;
    atomic_store(&a->stop,1);if(a->started)pthread_join(a->thread,NULL);
    if(a->pcm){a->drop(a->pcm);a->close(a->pcm);}
    if(a->library)dlclose(a->library);free(a);
}
void* halls_audio_open(halls_callback render,void* user) {
    Audio* a=(Audio*)calloc(1,sizeof(Audio));if(!a)return NULL;
    a->render=render;a->user=user;a->library=dlopen("libasound.so.2",RTLD_NOW|RTLD_LOCAL);
    if(!a->library)goto fail;
#define LOAD(field,name) *(void**)(&a->field)=dlsym(a->library,name);if(!a->field)goto fail
    LOAD(open,"snd_pcm_open");LOAD(params,"snd_pcm_set_params");LOAD(write,"snd_pcm_writei");
    LOAD(prepare,"snd_pcm_prepare");LOAD(drop,"snd_pcm_drop");LOAD(close,"snd_pcm_close");
#undef LOAD
    /* Playback, nonblocking; signed 16-bit LE, interleaved, 30ms target latency. */
    if(a->open(&a->pcm,"default",0,1)<0)goto fail;
    if(a->params(a->pcm,2,3,CHANNELS,48000,1,30000)<0)goto fail;
    atomic_store(&a->running,1);
    if(pthread_create(&a->thread,NULL,worker,a)!=0)goto fail;
    a->started=1;return a;
fail:halls_audio_close(a);return NULL;
}
int halls_audio_running(void* opaque){Audio* a=(Audio*)opaque;return a && atomic_load(&a->running);}
#endif
