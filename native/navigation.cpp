#include "navigation.h"
#include "DetourNavMesh.h"
#include "DetourNavMeshQuery.h"
#include "DetourAlloc.h"
#include <fstream>
#include <vector>
#include <memory>
#include <stdexcept>
#include <cmath>
#include <cstring>
#include <cstdio>
namespace {
void need(bool b,const char* why){if(!b)throw std::runtime_error(why);}
struct Header{uint32_t magic,detour,version,size,liquids;};
std::vector<unsigned char> read(const std::string& name,size_t limit){std::ifstream f(name,std::ios::binary|std::ios::ate);if(!f)return {};auto n=f.tellg();need(n>=0&&size_t(n)<=limit,"navigation file too large");std::vector<unsigned char>b(size_t(n),0);f.seekg(0);need(bool(f.read(reinterpret_cast<char*>(b.data()),n)),"navigation read failed");return b;}
float distance(const float*a,const float*b){float x=a[0]-b[0],y=a[1]-b[1],z=a[2]-b[2];return std::sqrt(x*x+y*y+z*z);}
}
extern "C" int ra_nav_path(const char* root,uint32_t map,const float* start,const float* end,float* output,int capacity,char* error,size_t error_capacity){try{
 need(root&&start&&end&&output&&capacity>=2&&capacity<=4096&&map<=999,"invalid navigation arguments");for(int i=0;i<3;++i)need(std::isfinite(start[i])&&std::isfinite(end[i])&&std::fabs(start[i])<20000&&std::fabs(end[i])<20000,"invalid coordinates");
 char name[64];std::snprintf(name,sizeof(name),"/%03u.mmap",map);auto params=read(std::string(root)+name,sizeof(dtNavMeshParams));need(params.size()==sizeof(dtNavMeshParams),"missing or incompatible mmap params");dtNavMeshParams p;std::memcpy(&p,params.data(),sizeof p);
 need(std::isfinite(p.tileWidth)&&p.tileWidth>0&&p.tileWidth<1000&&p.maxTiles>0&&p.maxTiles<=16384&&p.maxPolys>=0&&p.maxPolys<=1048576,"invalid mmap parameters");
 std::unique_ptr<dtNavMesh,void(*)(dtNavMesh*)> mesh(dtAllocNavMesh(),dtFreeNavMesh);need(bool(mesh),"mesh allocation failed");need(dtStatusSucceed(mesh->init(&p)),"mesh init failed");
 // MMap filenames are map + grid-y + grid-x; world coordinates reverse grid direction.
 int ax=int(32-start[0]/533.333333f),ay=int(32-start[1]/533.333333f),bx=int(32-end[0]/533.333333f),by=int(32-end[1]/533.333333f);
 int xmin=std::max(0,std::min(ax,bx)-1),xmax=std::min(63,std::max(ax,bx)+1),ymin=std::max(0,std::min(ay,by)-1),ymax=std::min(63,std::max(ay,by)+1);need((xmax-xmin+1)*(ymax-ymin+1)<=64,"route exceeds tile budget");
 unsigned loaded=0;for(int x=xmin;x<=xmax;++x)for(int y=ymin;y<=ymax;++y){std::snprintf(name,sizeof(name),"/%03u%02d%02d.mmtile",map,x,y);auto b=read(std::string(root)+name,16*1024*1024);if(b.empty())continue;need(b.size()>=sizeof(Header),"truncated tile");Header h;std::memcpy(&h,b.data(),sizeof h);need(h.magic==0x4d4d4150&&h.version==6&&h.detour==DT_NAVMESH_VERSION&&h.size==b.size()-sizeof h&&h.size>=sizeof(dtMeshHeader),"incompatible or corrupt tile");auto* bytes=static_cast<unsigned char*>(dtAlloc(h.size,DT_ALLOC_PERM));need(bytes,"tile allocation failed");std::memcpy(bytes,b.data()+sizeof h,h.size);auto result=mesh->addTile(bytes,int(h.size),DT_TILE_FREE_DATA,0,nullptr);if(dtStatusFailed(result)){dtFree(bytes);throw std::runtime_error("tile rejected");}++loaded;}
 need(loaded>0,"no tiles for route");std::unique_ptr<dtNavMeshQuery,void(*)(dtNavMeshQuery*)> query(dtAllocNavMeshQuery(),dtFreeNavMeshQuery);need(bool(query)&&dtStatusSucceed(query->init(mesh.get(),8192)),"query init failed");dtQueryFilter filter;filter.setIncludeFlags(1);filter.setExcludeFlags(2|4|8|16);
 float a[3]={start[1],start[2],start[0]},b[3]={end[1],end[2],end[0]},extent[3]={2,3,2},pa[3],pb[3];dtPolyRef ar=0,br=0;need(dtStatusSucceed(query->findNearestPoly(a,extent,&filter,&ar,pa))&&ar&&dtStatusSucceed(query->findNearestPoly(b,extent,&filter,&br,pb))&&br,"no walkable start or destination");need(distance(a,pa)<=3&&distance(b,pb)<=3,"projection too far");
 dtPolyRef corridor[4096];int count=0;auto status=query->findPath(ar,br,pa,pb,&filter,corridor,&count,4096);need(dtStatusSucceed(status)&&!(status&(DT_PARTIAL_RESULT|DT_BUFFER_TOO_SMALL|DT_OUT_OF_NODES))&&count>0&&corridor[count-1]==br,"no complete route");
 for(int i=0;i<count;++i){const dtMeshTile*t=nullptr;const dtPoly*poly=nullptr;need(dtStatusSucceed(mesh->getTileAndPolyByRef(corridor[i],&t,&poly))&&poly->getType()!=DT_POLYTYPE_OFFMESH_CONNECTION,"offmesh route forbidden");}
 std::vector<float> points(size_t(capacity)*3);int n=0;status=query->findStraightPath(pa,pb,corridor,count,points.data(),nullptr,nullptr,&n,capacity,DT_STRAIGHTPATH_ALL_CROSSINGS);need(dtStatusSucceed(status)&&!(status&DT_BUFFER_TOO_SMALL)&&n>=2&&distance(points.data()+3*(n-1),pb)<.5f,"incomplete straight route");
 for(int i=0;i<n;++i){output[3*i]=points[3*i+2];output[3*i+1]=points[3*i];output[3*i+2]=points[3*i+1];}return n;
 }catch(const std::exception&e){if(error&&error_capacity)std::snprintf(error,error_capacity,"%s",e.what());return -1;}catch(...){if(error&&error_capacity)std::snprintf(error,error_capacity,"navigation failure");return -1;}}
