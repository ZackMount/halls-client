/* macOS adapter: public Security and CommonCrypto APIs, no Swift runtime. */
#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <CommonCrypto/CommonCryptor.h>
#include <CommonCrypto/CommonDigest.h>
static void wipe(void* p,size_t n){volatile unsigned char* v=p;while(n--)*v++=0;}
static SecKeyRef apple_key(const unsigned char* p,size_t n,CFTypeRef type,CFTypeRef kind){
 const void* names[]={kSecAttrKeyType,kSecAttrKeyClass};const void* values[]={type,kind};
 CFDictionaryRef attrs=CFDictionaryCreate(NULL,names,values,2,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);
 /* A key provider may retain its import data. Own this small key encoding,
    because the caller's private-key scratch is wiped after key creation. */
 CFDataRef data=CFDataCreate(NULL,p,n);SecKeyRef key=NULL;
 /* The error output is NULL, so no owned CFError is created on failures. */
 if(data&&attrs)key=SecKeyCreateWithData(data,attrs,NULL);
 if(data)CFRelease(data);if(attrs)CFRelease(attrs);return key;
}
typedef struct {CC_SHA256_CTX hash;int finished;} ProofDigest;
void halls_digest_close(void* ptr){if(ptr){wipe(ptr,sizeof(ProofDigest));free(ptr);}}
void* halls_digest_open(void){ProofDigest* c=calloc(1,sizeof(*c));if(c)CC_SHA256_Init(&c->hash);return c;}
int halls_digest_update(void* ptr,const unsigned char* p,uint32_t n){ProofDigest* c=ptr;return c&&!c->finished&&n<=4096&&CC_SHA256_Update(&c->hash,p,n)?0:-2;}
int halls_digest_finish(void* ptr,unsigned char* out){
 ProofDigest* c=ptr;if(!c||c->finished)return -2;c->finished=1;
 unsigned char hash[32],raw[97],material[PROOF_SIGNING_SIZE];CC_SHA256_Final(hash,&c->hash);proof_signing_material(material);raw[0]=4;memcpy(raw+1,material+8,96);wipe(material,sizeof(material));
 SecKeyRef key=apple_key(raw,sizeof(raw),kSecAttrKeyTypeECSECPrimeRandom,kSecAttrKeyClassPrivate);wipe(raw,sizeof(raw));
 if(!key){wipe(hash,sizeof(hash));return -2;}
 CFDataRef data=CFDataCreateWithBytesNoCopy(NULL,hash,32,kCFAllocatorNull),signature=NULL;
 if(data)signature=SecKeyCreateSignature(key,kSecKeyAlgorithmECDSASignatureDigestX962SHA256,data,NULL);
 if(data)CFRelease(data);CFRelease(key);wipe(hash,sizeof(hash));if(!signature)return -2;
 const unsigned char* der=CFDataGetBytePtr(signature);size_t count=CFDataGetLength(signature),at=2;int ok=0;
 if(count>=8&&count<=72&&der[0]==0x30&&der[1]==count-2){
  memset(out,0,64);ok=1;
  for(unsigned part=0;part<2;part++){
   if(at+2>count||der[at++]!=2){ok=0;break;}size_t len=der[at++];
   if(!len||len>33||at+len>count||(len==33&&der[at]!=0)){ok=0;break;}
   if(len==33){at++;len--;}memcpy(out+part*32+32-len,der+at,len);at+=len;
  }
  if(at!=count)ok=0;
 }
 CFRelease(signature);return ok?0:-2;
}
/* NIST SP 800-38D GHASH. Fixed rounds and addresses; key-dependent choices use
   unsigned masks. AES itself is the public system implementation. */
static void multiply(unsigned char* y,const unsigned char* h){
 uint64_t state[6]={0};memcpy(state,y,16);memcpy(state+2,h,16);
 for(unsigned i=0;i<4;i++)state[i]=__builtin_bswap64(state[i]);
 for(unsigned i=0;i<128;i++){
  uint64_t mask=0-(state[0]>>63);state[4]^=state[2]&mask;state[5]^=state[3]&mask;
  state[0]=(state[0]<<1)|(state[1]>>63);state[1]<<=1;
  mask=0-(state[3]&1);state[3]=(state[3]>>1)|(state[2]<<63);
  state[2]=(state[2]>>1)^(UINT64_C(0xe100000000000000)&mask);
 }
 state[4]=__builtin_bswap64(state[4]);state[5]=__builtin_bswap64(state[5]);
 memcpy(y,state+4,16);wipe(state,sizeof(state));
}
static void hash_bytes(unsigned char* y,const unsigned char* h,const unsigned char* p,size_t n){
 while(n){size_t block=n<16?n:16;for(size_t i=0;i<block;i++)y[i]^=p[i];multiply(y,h);p+=block;n-=block;}
}
static int aes_block(CCCryptorRef aes,const unsigned char* p,unsigned char* out){size_t n=0;return CCCryptorUpdate(aes,p,16,out,16,&n)==kCCSuccess&&n==16;}
typedef struct {CCCryptorRef aes;unsigned char h[16],counter[16],y[16],tag_base[16];unsigned char* tag;uint32_t expected,written;int failed,finished;} ProofEnvelope;
void halls_envelope_close(void* ptr){ProofEnvelope* c=ptr;if(!c)return;if(c->aes)CCCryptorRelease(c->aes);wipe(c,sizeof(*c));free(c);}
void* halls_envelope_open(uint32_t n,uint32_t plain,unsigned char* out){
 ProofEnvelope* c=calloc(1,sizeof(*c));if(!c)return NULL;
 unsigned char secret[32],der[270];int ok=0;SecKeyRef key=NULL;CFDataRef data=NULL,wrapped=NULL;
 memcpy(der,"\x30\x82\x01\x0a\x02\x82\x01\x01\0",9);memcpy(der+9,proof_recipient_key+27,256);der[265]=2;der[266]=3;memcpy(der+267,proof_recipient_key+24,3);
 key=apple_key(der,sizeof(der),kSecAttrKeyTypeRSA,kSecAttrKeyClassPublic);if(!key||!n)goto done;
 memcpy(out,"HLPF\1\r\n\32",8);memcpy(out+8,proof_key_id,8);memcpy(out+28,&plain,4);memcpy(out+32,&n,4);
 if(SecRandomCopyBytes(kSecRandomDefault,32,secret)||SecRandomCopyBytes(kSecRandomDefault,12,out+16))goto done;
 data=CFDataCreateWithBytesNoCopy(NULL,secret,32,kCFAllocatorNull);if(!data)goto done;
 wrapped=SecKeyCreateEncryptedData(key,kSecKeyAlgorithmRSAEncryptionOAEPSHA256,data,NULL);
 if(!wrapped||CFDataGetLength(wrapped)!=256)goto done;memcpy(out+36,CFDataGetBytePtr(wrapped),256);
 if(CCCryptorCreate(kCCEncrypt,kCCAlgorithmAES,kCCOptionECBMode,secret,32,NULL,&c->aes)!=kCCSuccess)goto done;
 memcpy(c->counter,out+16,12);c->counter[15]=1;
 if(!aes_block(c->aes,c->h,c->h)||!aes_block(c->aes,c->counter,c->tag_base))goto done;
 hash_bytes(c->y,c->h,out,292);c->tag=out+292;c->expected=n;ok=1;
 done:if(wrapped)CFRelease(wrapped);if(data)CFRelease(data);if(key)CFRelease(key);wipe(secret,sizeof(secret));if(!ok){halls_envelope_close(c);return NULL;}return c;
}
int halls_envelope_update(void* ptr,const unsigned char* p,uint32_t n,unsigned char* out){
 ProofEnvelope* c=ptr;unsigned char block[16];if(!c||c->failed||c->finished||!n||n>4096||n>c->expected-c->written)return -2;
 if(n!=c->expected-c->written&&n%16)return -2;
 for(uint32_t at=0;at<n;at+=16){
  for(int k=15;k>=12;k--)if(++c->counter[k])break;
  if(!aes_block(c->aes,c->counter,block)){wipe(block,sizeof(block));c->failed=1;return -2;}
  uint32_t length=n-at<16?n-at:16;for(uint32_t k=0;k<length;k++)out[at+k]=p[at+k]^block[k];
 }
 hash_bytes(c->y,c->h,out,n);c->written+=n;wipe(block,sizeof(block));return 0;
}
int halls_envelope_finish(void* ptr){
 ProofEnvelope* c=ptr;if(!c||c->failed||c->finished||c->written!=c->expected)return -2;c->finished=1;
 unsigned char sizes[16];uint64_t a=292*8,b=(uint64_t)c->expected*8;for(unsigned k=0;k<8;k++){sizes[7-k]=(unsigned char)(a>>(k*8));sizes[15-k]=(unsigned char)(b>>(k*8));}
 hash_bytes(c->y,c->h,sizes,16);for(unsigned k=0;k<16;k++)c->tag[k]=c->tag_base[k]^c->y[k];wipe(sizes,sizeof(sizes));return 0;
}
