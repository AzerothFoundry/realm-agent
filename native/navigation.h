#ifndef REALM_AGENT_NAVIGATION_H
#define REALM_AGENT_NAVIGATION_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
int ra_nav_path(const char* root,uint32_t map,const float* start,const float* end,float* output,int capacity,char* error,size_t error_capacity);
#ifdef __cplusplus
}
#endif
#endif
