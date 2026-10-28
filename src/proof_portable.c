/* Production Unix adapter. No organizer decryption key is present here. */
#include "proof_material.h"
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <limits.h>
#include <stdlib.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#include "proof_apple.c"
#else
#include <openssl/evp.h>
#include <openssl/ec.h>
#include <openssl/rsa.h>
#include <openssl/sha.h>
#include <openssl/rand.h>
typedef struct {EVP_MD_CTX* hash;int finished,failed;} ProofDigest;
void halls_digest_close(void* ptr){ProofDigest* c=ptr;if(!c)return;EVP_MD_CTX_free(c->hash);OPENSSL_cleanse(c,sizeof(*c));free(c);}
void* halls_digest_open(void){ProofDigest* c=calloc(1,sizeof(*c));if(!c)return NULL;c->hash=EVP_MD_CTX_new();if(!c->hash||EVP_DigestInit_ex(c->hash,EVP_sha256(),NULL)!=1){halls_digest_close(c);return NULL;}return c;}
int halls_digest_update(void* ptr,const unsigned char* p,uint32_t n){ProofDigest* c=ptr;if(!c||c->finished||c->failed||n>4096)return -2;if(EVP_DigestUpdate(c->hash,p,n)!=1){c->failed=1;return -2;}return 0;}
int halls_digest_finish(void* ptr,unsigned char* out){
 ProofDigest* c=ptr;if(!c||c->finished||c->failed)return -2;c->finished=1;
 int ok=0;unsigned char hash[32];unsigned got=0;EC_KEY* key=EC_KEY_new_by_curve_name(NID_X9_62_prime256v1);
 unsigned char material[PROOF_SIGNING_SIZE];proof_signing_material(material);BIGNUM* d=BN_bin2bn(material+72,32,NULL);OPENSSL_cleanse(material,sizeof(material));ECDSA_SIG* sig=NULL;EC_POINT* point=NULL;
 if(!key||!d||EVP_DigestFinal_ex(c->hash,hash,&got)!=1||got!=32)goto done;
 const EC_GROUP* group=EC_KEY_get0_group(key);point=EC_POINT_new(group);
 if(!point||!EC_POINT_mul(group,point,d,NULL,NULL,NULL)||!EC_KEY_set_private_key(key,d)||!EC_KEY_set_public_key(key,point))goto done;
 sig=ECDSA_do_sign(hash,32,key);if(!sig)goto done;
 const BIGNUM *r,*s;ECDSA_SIG_get0(sig,&r,&s);ok=BN_bn2binpad(r,out,32)==32&&BN_bn2binpad(s,out+32,32)==32;
 done:OPENSSL_cleanse(hash,sizeof(hash));EC_POINT_free(point);ECDSA_SIG_free(sig);EC_KEY_free(key);BN_clear_free(d);return ok?0:-2;
}
typedef struct {EVP_CIPHER_CTX* aes;unsigned char* tag;uint32_t expected,written;int failed,finished;} ProofEnvelope;
void halls_envelope_close(void* ptr){ProofEnvelope* c=ptr;if(!c)return;EVP_CIPHER_CTX_free(c->aes);OPENSSL_cleanse(c,sizeof(*c));free(c);}
void* halls_envelope_open(uint32_t n,uint32_t plain,unsigned char* out){
 ProofEnvelope* c=calloc(1,sizeof(*c));if(!c)return NULL;
 int ok=0,got=0;size_t wrapped=256;unsigned char secret[32];
 RSA* rsa=RSA_new();EVP_PKEY* pub=EVP_PKEY_new();EVP_PKEY_CTX* wrap=NULL;
 BIGNUM* exponent=BN_bin2bn(proof_recipient_key+24,3,NULL);BIGNUM* modulus=BN_bin2bn(proof_recipient_key+27,256,NULL);
 if(!n||!rsa||!pub||!exponent||!modulus||!RSA_set0_key(rsa,modulus,exponent,NULL))goto done;
 exponent=modulus=NULL;if(!EVP_PKEY_assign_RSA(pub,rsa))goto done;rsa=NULL;
 memcpy(out,"HLPF\1\r\n\32",8);memcpy(out+8,proof_key_id,8);memcpy(out+28,&plain,4);memcpy(out+32,&n,4);
 if(RAND_bytes(secret,32)!=1||RAND_bytes(out+16,12)!=1)goto done;
 wrap=EVP_PKEY_CTX_new(pub,NULL);if(!wrap||EVP_PKEY_encrypt_init(wrap)<=0||EVP_PKEY_CTX_set_rsa_padding(wrap,RSA_PKCS1_OAEP_PADDING)<=0||EVP_PKEY_CTX_set_rsa_oaep_md(wrap,EVP_sha256())<=0||EVP_PKEY_CTX_set_rsa_mgf1_md(wrap,EVP_sha256())<=0||EVP_PKEY_encrypt(wrap,out+36,&wrapped,secret,32)<=0||wrapped!=256)goto done;
 c->aes=EVP_CIPHER_CTX_new();if(!c->aes||EVP_EncryptInit_ex(c->aes,EVP_aes_256_gcm(),NULL,secret,out+16)!=1||EVP_EncryptUpdate(c->aes,NULL,&got,out,292)!=1)goto done;
 c->tag=out+292;c->expected=n;ok=1;
 done:OPENSSL_cleanse(secret,sizeof(secret));BN_free(exponent);BN_free(modulus);RSA_free(rsa);EVP_PKEY_free(pub);EVP_PKEY_CTX_free(wrap);if(!ok){halls_envelope_close(c);return NULL;}return c;
}
int halls_envelope_update(void* ptr,const unsigned char* p,uint32_t n,unsigned char* out){ProofEnvelope* c=ptr;int got=0;if(!c||c->failed||c->finished||!n||n>4096||n>c->expected-c->written)return -2;if(EVP_EncryptUpdate(c->aes,out,&got,p,n)!=1||got!=(int)n){c->failed=1;return -2;}c->written+=n;return 0;}
int halls_envelope_finish(void* ptr){ProofEnvelope* c=ptr;unsigned char tail[16];int n=0;if(!c||c->failed||c->finished||c->written!=c->expected)return -2;c->finished=1;int ok=EVP_EncryptFinal_ex(c->aes,tail,&n)==1&&!n&&EVP_CIPHER_CTX_ctrl(c->aes,EVP_CTRL_GCM_GET_TAG,16,c->tag)==1;OPENSSL_cleanse(tail,sizeof(tail));return ok?0:-2;}

#endif
int halls_proof_write(const unsigned char* name,const unsigned char* p,uint32_t n){
    char path[4096];size_t at;
#ifdef __APPLE__
    uint32_t capacity=sizeof(path);if(_NSGetExecutablePath(path,&capacity))return -3;at=strlen(path);
#else
    ssize_t count=readlink("/proc/self/exe",path,sizeof(path)-1);if(count<=0)return -3;at=(size_t)count;path[at]=0;
#endif
    while(at&&path[at-1]!='/')--at;
    size_t length=strlen((const char*)name);if(!at||at+length>=sizeof(path))return -3;memcpy(path+at,name,length+1);
    int fd=open(path,O_WRONLY|O_CREAT|O_EXCL,0600);if(fd<0)return errno;
    int error=0;while(n){ssize_t wrote=write(fd,p,n);if(wrote<0&&errno==EINTR)continue;if(wrote<=0){error=-4;break;}p+=wrote;n-=(uint32_t)wrote;}
    if(fsync(fd))error=-4;if(close(fd))error=-4;if(error)unlink(path);return error;
}
