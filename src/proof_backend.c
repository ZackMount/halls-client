/* Windows uses only the already linked OS cryptographic provider. */
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <bcrypt.h>
#include "proof_material.h"
/* Windows x64 KUSER_SHARED_DATA: read the kernel-published clocks directly.
   KSYSTEM_TIME high1/low/high2 sampling prevents torn rollover reads. */
static uint64_t shared_time(unsigned offset){
    volatile const uint32_t* p=(volatile const uint32_t*)(uintptr_t)(0x7ffe0000u+offset);
    uint32_t high,low;do{high=p[1];low=p[0];}while(high!=p[2]);
    return ((uint64_t)high<<32)|low;
}
void halls_proof_clock(uint64_t* out){out[0]=shared_time(8)/10;out[1]=(shared_time(20)-116444736000000000ULL)/10;
    ULONGLONG awake=0;QueryUnbiasedInterruptTime(&awake);out[2]=awake/10;}
void halls_proof_time(unsigned char* out){
    FILETIME now; TIME_ZONE_INFORMATION zone;
    GetSystemTimeAsFileTime(&now);
    uint64_t seconds=(((uint64_t)now.dwHighDateTime<<32)|now.dwLowDateTime)/10000000-11644473600ULL;
    DWORD mode=GetTimeZoneInformation(&zone);
    int32_t offset=mode==TIME_ZONE_ID_INVALID?2147483647:-(zone.Bias+(mode==TIME_ZONE_ID_DAYLIGHT?zone.DaylightBias:mode==TIME_ZONE_ID_STANDARD?zone.StandardBias:0));
    memcpy(out,&seconds,8);memcpy(out+8,&offset,4);
}
static int open_alg(BCRYPT_ALG_HANDLE* h,LPCWSTR name){return BCryptOpenAlgorithmProvider(h,name,0,0)>=0;}
typedef struct {BCRYPT_ALG_HANDLE algorithm;BCRYPT_HASH_HANDLE hash;int finished,failed;} ProofDigest;
void halls_digest_close(void* ptr){ProofDigest* c=ptr;if(!c)return;if(c->hash)BCryptDestroyHash(c->hash);if(c->algorithm)BCryptCloseAlgorithmProvider(c->algorithm,0);SecureZeroMemory(c,sizeof(*c));HeapFree(GetProcessHeap(),0,c);}
void* halls_digest_open(void){
 ProofDigest* c=HeapAlloc(GetProcessHeap(),HEAP_ZERO_MEMORY,sizeof(*c));if(!c)return 0;
 if(!open_alg(&c->algorithm,BCRYPT_SHA256_ALGORITHM)||BCryptCreateHash(c->algorithm,&c->hash,0,0,0,0,0)<0){halls_digest_close(c);return 0;}return c;
}
int halls_digest_update(void* ptr,const unsigned char* p,uint32_t n){ProofDigest* c=ptr;if(!c||c->finished||c->failed||n>4096)return -2;if(BCryptHashData(c->hash,(PUCHAR)p,n,0)<0){c->failed=1;return -2;}return 0;}
int halls_digest_finish(void* ptr,unsigned char* signature){
 ProofDigest* c=ptr;BCRYPT_ALG_HANDLE ec=0;BCRYPT_KEY_HANDLE key=0;unsigned char digest[32];ULONG got=0;int ok=0;
 if(!c||c->finished||c->failed)return -2;c->finished=1;
 if(BCryptFinishHash(c->hash,digest,32,0)<0||!open_alg(&ec,BCRYPT_ECDSA_P256_ALGORITHM))goto done;
 unsigned char material[PROOF_SIGNING_SIZE];proof_signing_material(material);NTSTATUS imported=BCryptImportKeyPair(ec,0,BCRYPT_ECCPRIVATE_BLOB,&key,material,sizeof(material),0);SecureZeroMemory(material,sizeof(material));if(imported<0)goto done;
 ok=BCryptSignHash(key,0,digest,32,signature,64,&got,0)>=0&&got==64;
 done:SecureZeroMemory(digest,sizeof(digest));if(key)BCryptDestroyKey(key);if(ec)BCryptCloseAlgorithmProvider(ec,0);return ok?0:-2;
}
typedef struct {BCRYPT_ALG_HANDLE algorithm;BCRYPT_KEY_HANDLE key;BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO auth;unsigned char mac[16],iv[16];uint32_t expected,written;int failed,finished;} ProofEnvelope;
void halls_envelope_close(void* ptr){ProofEnvelope* c=ptr;if(!c)return;if(c->key)BCryptDestroyKey(c->key);if(c->algorithm)BCryptCloseAlgorithmProvider(c->algorithm,0);SecureZeroMemory(c,sizeof(*c));HeapFree(GetProcessHeap(),0,c);}
void* halls_envelope_open(uint32_t n,uint32_t plain,unsigned char* out){
 ProofEnvelope* c=HeapAlloc(GetProcessHeap(),HEAP_ZERO_MEMORY,sizeof(*c));BCRYPT_ALG_HANDLE rsa=0;BCRYPT_KEY_HANDLE pub=0;unsigned char secret[32];ULONG got=0;int ok=0;
 BCRYPT_OAEP_PADDING_INFO oaep={BCRYPT_SHA256_ALGORITHM,0,0};if(!c)return 0;
 memcpy(out,"HLPF\1\r\n\32",8);memcpy(out+8,proof_key_id,8);memcpy(out+28,&plain,4);memcpy(out+32,&n,4);
 if(!n||BCryptGenRandom(0,secret,32,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0||BCryptGenRandom(0,out+16,12,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0)goto done;
 if(!open_alg(&rsa,BCRYPT_RSA_ALGORITHM)||!open_alg(&c->algorithm,BCRYPT_AES_ALGORITHM))goto done;
 if(BCryptImportKeyPair(rsa,0,BCRYPT_RSAPUBLIC_BLOB,&pub,(PUCHAR)proof_recipient_key,sizeof(proof_recipient_key),0)<0)goto done;
 if(BCryptEncrypt(pub,secret,32,&oaep,0,0,out+36,256,&got,BCRYPT_PAD_OAEP)<0||got!=256)goto done;
 if(BCryptSetProperty(c->algorithm,BCRYPT_CHAINING_MODE,(PUCHAR)BCRYPT_CHAIN_MODE_GCM,sizeof(BCRYPT_CHAIN_MODE_GCM),0)<0||BCryptGenerateSymmetricKey(c->algorithm,&c->key,0,0,secret,32,0)<0)goto done;
 BCRYPT_INIT_AUTH_MODE_INFO(c->auth);c->auth.pbNonce=out+16;c->auth.cbNonce=12;c->auth.pbAuthData=out;c->auth.cbAuthData=292;c->auth.pbTag=out+292;c->auth.cbTag=16;c->auth.pbMacContext=c->mac;c->auth.cbMacContext=16;c->auth.dwFlags=BCRYPT_AUTH_MODE_CHAIN_CALLS_FLAG;c->expected=n;ok=1;
 done:SecureZeroMemory(secret,sizeof(secret));if(pub)BCryptDestroyKey(pub);if(rsa)BCryptCloseAlgorithmProvider(rsa,0);if(!ok){halls_envelope_close(c);return 0;}return c;
}
int halls_envelope_update(void* ptr,const unsigned char* p,uint32_t n,unsigned char* out){
 ProofEnvelope* c=ptr;ULONG got=0;if(!c||c->failed||c->finished||!n||n>4096||n>c->expected-c->written)return -2;
 int last=n==c->expected-c->written;if(!last&&n%16)return -2;
 if(last)c->auth.dwFlags&=~BCRYPT_AUTH_MODE_CHAIN_CALLS_FLAG;
 if(BCryptEncrypt(c->key,(PUCHAR)p,n,&c->auth,c->iv,16,out,n,&got,0)<0||got!=n){c->failed=1;return -2;}
 c->written+=n;c->auth.pbAuthData=0;c->auth.cbAuthData=0;return 0;
}
int halls_envelope_finish(void* ptr){ProofEnvelope* c=ptr;if(!c||c->failed||c->finished||c->written!=c->expected)return -2;c->finished=1;return 0;}
int halls_proof_write(const unsigned char* name,const unsigned char* data,uint32_t size){
    WCHAR path[1024];DWORD n=GetModuleFileNameW(0,path,1024),wrote=0;int result=0;HANDLE file;
    if(!n||n>=1024)return -3;
    while(n && path[n-1]!=L'\\' && path[n-1]!=L'/')--n;
    if(!n || !MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,(LPCCH)name,-1,path+n,1024-n))return -3;
    file=CreateFileW(path,GENERIC_WRITE,0,0,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,0);
    if(file==INVALID_HANDLE_VALUE)return (int)GetLastError();
    if(!WriteFile(file,data,size,&wrote,0)||wrote!=size||!FlushFileBuffers(file))result=-4;
    CloseHandle(file);if(result)DeleteFileW(path);return result;
}
#else
#include <time.h>
#ifdef __APPLE__
#include <mach/mach_time.h>
void halls_proof_clock(uint64_t* out){
    mach_timebase_info_data_t base;struct timespec utc;
    mach_timebase_info(&base);
    uint64_t ticks=mach_continuous_time();
    out[0]=(ticks/base.denom*base.numer+(ticks%base.denom)*base.numer/base.denom)/1000;
    clock_gettime(CLOCK_REALTIME,&utc);out[1]=(uint64_t)utc.tv_sec*1000000u+utc.tv_nsec/1000;
    ticks=mach_absolute_time();out[2]=(ticks/base.denom*base.numer+(ticks%base.denom)*base.numer/base.denom)/1000;
}
#else
#include <unistd.h>
#include <sys/syscall.h>
void halls_proof_clock(uint64_t* out){struct timespec t;
    syscall(SYS_clock_gettime,CLOCK_BOOTTIME,&t);out[0]=(uint64_t)t.tv_sec*1000000u+t.tv_nsec/1000;
    syscall(SYS_clock_gettime,CLOCK_REALTIME,&t);out[1]=(uint64_t)t.tv_sec*1000000u+t.tv_nsec/1000;
    syscall(SYS_clock_gettime,CLOCK_MONOTONIC_RAW,&t);out[2]=(uint64_t)t.tv_sec*1000000u+t.tv_nsec/1000;
}
#endif
void halls_proof_time(unsigned char* out){
    time_t now=time(0);struct tm local;localtime_r(&now,&local);
    int64_t seconds=(int64_t)now;int32_t offset=(int32_t)(local.tm_gmtoff/60);
    memcpy(out,&seconds,8);memcpy(out+8,&offset,4);
}
#include "proof_portable.c"
#endif
