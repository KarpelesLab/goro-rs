/* Multi-connection script runner (C twin of the shared-cache tests in
** goro-sqlite/src/btree.rs).  Script lines:
**   @open N FILENAME          sqlite3_open_v2(RW|CREATE|URI)
**   @openshared N FILENAME    same with SQLITE_OPEN_SHAREDCACHE
**   @close N
**   @enable B                 sqlite3_enable_shared_cache(B)
**   @prep N NAME SQL          prepare and keep a statement
**   @step NAME                step it once, print row / status
**   @fin NAME                 finalize
**   N: SQL                    run all statements of SQL on connection N
** Output: "N: a|b" rows, "N: Error: rc ext msg".
*/
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "sqlite3.h"

static sqlite3 *aDb[10];
static struct { char name[32]; sqlite3_stmt *st; int conn; } aSt[32];

static void row(int n, sqlite3_stmt *st){
  int i, nc = sqlite3_column_count(st);
  printf("%d: ", n);
  for(i=0;i<nc;i++){
    if( i ) putchar('|');
    if( sqlite3_column_type(st,i)!=SQLITE_NULL ) fwrite(sqlite3_column_text(st,i),1,sqlite3_column_bytes(st,i),stdout);
  }
  putchar('\n');
}
static void err(int n, int rc){
  printf("%d: Error: %d %d %s\n", n, rc, sqlite3_extended_errcode(aDb[n]), sqlite3_errmsg(aDb[n]));
}

int main(int argc, char **argv){
  char line[4096];
  FILE *in = argc>1 ? fopen(argv[1],"r") : stdin;
  while( fgets(line, sizeof(line), in) ){
    int n; char a[256], b[256];
    size_t L = strlen(line);
    while( L && (line[L-1]=='\n'||line[L-1]=='\r') ) line[--L] = 0;
    if( L==0 || line[0]=='#' ) continue;
    if( sscanf(line, "@open %d %255s", &n, a)==2 || sscanf(line, "@openshared %d %255s", &n, a)==2 ){
      int fl = SQLITE_OPEN_READWRITE|SQLITE_OPEN_CREATE|SQLITE_OPEN_URI;
      if( strncmp(line,"@openshared",11)==0 ) fl |= SQLITE_OPEN_SHAREDCACHE;
      int rc = sqlite3_open_v2(a, &aDb[n], fl, 0);
      if( rc ) printf("%d: open error %d\n", n, rc);
    }else if( sscanf(line, "@close %d", &n)==1 ){
      printf("%d: close %d\n", n, sqlite3_close(aDb[n]));
      aDb[n] = 0;
    }else if( sscanf(line, "@enable %d", &n)==1 ){
      sqlite3_enable_shared_cache(n);
    }else if( sscanf(line, "@prep %d %31s", &n, a)==2 ){
      char *z = strchr(line+6, ' '); z = strchr(z+1, ' ') + 1;
      int i; for(i=0; aSt[i].st; i++){}
      strcpy(aSt[i].name, a); aSt[i].conn = n;
      int rc = sqlite3_prepare_v2(aDb[n], z, -1, &aSt[i].st, 0);
      if( rc ){ err(n, rc); aSt[i].st = 0; }
    }else if( sscanf(line, "@step %31s", a)==1 ){
      int i; for(i=0; i<32 && (!aSt[i].st || strcmp(aSt[i].name,a)); i++){}
      if( i==32 ){ printf("no stmt %s\n", a); continue; }
      int rc = sqlite3_step(aSt[i].st);
      if( rc==SQLITE_ROW ) row(aSt[i].conn, aSt[i].st);
      else if( rc==SQLITE_DONE ) printf("%d: done\n", aSt[i].conn);
      else err(aSt[i].conn, rc);
    }else if( sscanf(line, "@fin %31s", a)==1 ){
      int i; for(i=0; i<32 && (!aSt[i].st || strcmp(aSt[i].name,a)); i++){}
      if( i==32 ){ printf("no stmt %s\n", a); continue; }
      sqlite3_finalize(aSt[i].st); aSt[i].st = 0;
    }else if( sscanf(line, "%d:", &n)==1 ){
      const char *z = strchr(line, ':') + 1;
      while( *z ){
        sqlite3_stmt *st = 0; const char *tail = 0;
        int rc = sqlite3_prepare_v2(aDb[n], z, -1, &st, &tail);
        if( rc ) err(n, rc);
        if( tail==0 || tail==z ) break;
        z = tail;
        if( !st ) continue;
        for(;;){
          rc = sqlite3_step(st);
          if( rc==SQLITE_ROW ) row(n, st);
          else { if( rc!=SQLITE_DONE ) err(n, rc); break; }
        }
        sqlite3_finalize(st);
      }
    }else{
      printf("bad line: %s\n", line);
    }
  }
  return 0;
}
