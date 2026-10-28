/* Narrow native console interface; presentation uses standard VT sequences. */
#include <stdint.h>
#include <stddef.h>
typedef struct {int kind,code,modifiers,event_kind,width,height;} Input;
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
static HANDLE input,output;static DWORD old_input,old_output,old_cp;static int active;
int halls_term_open(void) {
    input=GetStdHandle(STD_INPUT_HANDLE);output=GetStdHandle(STD_OUTPUT_HANDLE);
    if(!GetConsoleMode(input,&old_input)||!GetConsoleMode(output,&old_output))return -1;
    if(!SetConsoleMode(output,old_output|ENABLE_VIRTUAL_TERMINAL_PROCESSING|ENABLE_PROCESSED_OUTPUT))return -1;
    old_cp=GetConsoleOutputCP();
    if(!SetConsoleOutputCP(CP_UTF8)){SetConsoleMode(output,old_output);return -1;}
    if(!SetConsoleMode(input,ENABLE_WINDOW_INPUT|ENABLE_EXTENDED_FLAGS)){SetConsoleMode(output,old_output);SetConsoleOutputCP(old_cp);return -1;}
    active=1;return 0;
}
void halls_term_close(void){if(active){SetConsoleMode(input,old_input);SetConsoleMode(output,old_output);SetConsoleOutputCP(old_cp);active=0;}}
int halls_term_size(int* width,int* height){CONSOLE_SCREEN_BUFFER_INFO s;if(!GetConsoleScreenBufferInfo(output,&s))return -1;*width=s.srWindow.Right-s.srWindow.Left+1;*height=s.srWindow.Bottom-s.srWindow.Top+1;return 0;}
int halls_term_event(Input* event,int timeout) {
    DWORD started=GetTickCount();
    for(;;) {
        DWORD elapsed=GetTickCount()-started;
        DWORD wait=elapsed>=(DWORD)timeout?0:(DWORD)timeout-elapsed;
        DWORD result=WaitForSingleObject(input,wait);
        if(result==WAIT_TIMEOUT)return 0;
        if(result!=WAIT_OBJECT_0)return -1;
        INPUT_RECORD record;DWORD count;
        if(!ReadConsoleInputW(input,&record,1,&count)||!count)return -1;
        *event=(Input){0};
        if(record.EventType==KEY_EVENT){
            KEY_EVENT_RECORD k=record.Event.KeyEvent;
            event->kind=1;event->event_kind=k.bKeyDown?0:2;event->code=k.uChar.UnicodeChar;
            switch(k.wVirtualKeyCode){case VK_UP:event->code=256;break;case VK_DOWN:event->code=257;break;case VK_LEFT:event->code=258;break;case VK_RIGHT:event->code=259;break;}
            event->modifiers=((k.dwControlKeyState&SHIFT_PRESSED)?1:0)|((k.dwControlKeyState&(LEFT_CTRL_PRESSED|RIGHT_CTRL_PRESSED))?2:0)|((k.dwControlKeyState&(LEFT_ALT_PRESSED|RIGHT_ALT_PRESSED))?4:0);
            if(event->code==3 && (event->modifiers&2))event->code='c';
            if(event->code)return 1;
        }else if(record.EventType==WINDOW_BUFFER_SIZE_EVENT){event->kind=2;halls_term_size(&event->width,&event->height);return 1;}
        else if(record.EventType==FOCUS_EVENT){event->kind=record.Event.FocusEvent.bSetFocus?4:3;return 1;}
    }
}
#else
#include <unistd.h>
#include <termios.h>
#include <sys/ioctl.h>
#include <poll.h>
#include <errno.h>
static struct termios saved;static int active;
int halls_term_open(void){if(!isatty(STDIN_FILENO)||!isatty(STDOUT_FILENO))return -1;if(tcgetattr(STDIN_FILENO,&saved))return -1;struct termios raw=saved;cfmakeraw(&raw);raw.c_cc[VMIN]=1;raw.c_cc[VTIME]=0;if(tcsetattr(STDIN_FILENO,TCSANOW,&raw))return -1;active=1;return 0;}
void halls_term_close(void){if(active){tcsetattr(STDIN_FILENO,TCSANOW,&saved);active=0;}}
int halls_term_size(int* width,int* height){struct winsize s;if(ioctl(STDOUT_FILENO,TIOCGWINSZ,&s))return -1;*width=s.ws_col;*height=s.ws_row;return 0;}
int halls_term_bytes(unsigned char* bytes,int capacity,int timeout){struct pollfd fd={STDIN_FILENO,POLLIN,0};int n=poll(&fd,1,timeout);if(n<0)return errno==EINTR?0:-1;if(!n)return 0;if(fd.revents&(POLLERR|POLLHUP|POLLNVAL))return -1;n=(int)read(STDIN_FILENO,bytes,(size_t)capacity);return n>0?n:-1;}
#endif

int halls_write(int channel,const unsigned char* bytes,size_t length) {
#ifdef _WIN32
    HANDLE h=GetStdHandle(channel==2?STD_ERROR_HANDLE:STD_OUTPUT_HANDLE);
    DWORD cp=0,mode;int result=0;
    if(!active && GetConsoleMode(h,&mode)){cp=GetConsoleOutputCP();SetConsoleOutputCP(CP_UTF8);}
    while(length){DWORD written=0;DWORD count=length>0x7fffffffu?0x7fffffffu:(DWORD)length;
        if(!WriteFile(h,bytes,count,&written,NULL)||!written){result=-1;break;}
        bytes+=written;length-=written;}
    if(cp)SetConsoleOutputCP(cp);return result;
#else
    while(length){ssize_t n=write(channel,bytes,length);if(n<0 && errno==EINTR)continue;if(n<=0)return -1;bytes+=n;length-=(size_t)n;}return 0;
#endif
}

#ifdef _WIN32
#include <bcrypt.h>
int halls_random(uint64_t* value) {return BCryptGenRandom(NULL,(PUCHAR)value,sizeof(*value),BCRYPT_USE_SYSTEM_PREFERRED_RNG)>=0?0:-1;}
#elif defined(__APPLE__)
#include <stdlib.h>
int halls_random(uint64_t* value) {arc4random_buf(value,sizeof(*value));return 0;}
#else
#include <sys/random.h>
int halls_random(uint64_t* value) {
    unsigned char* p=(unsigned char*)value;size_t left=sizeof(*value);
    while(left){ssize_t n=getrandom(p,left,0);if(n<0&&errno==EINTR)continue;if(n<=0)return -1;p+=n;left-=(size_t)n;}return 0;
}
#endif
