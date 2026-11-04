/* Test-only caller of the unmodified ISDA CDS Standard Model 1.8.3.
 * This application is based on the ISDA CDS Standard Model (version 1.8.3),
 * developed and supported in collaboration with Markit Group Ltd.
 * Original model sources retain their separate public license. No model
 * implementation is linked into Convex's production dependency graph.
 */
#include <stdio.h>
#include <math.h>
#include "dateconv.h"
#include "date_sup.h"
#include "convert.h"
#include "cds.h"
#include "zerocurve.h"
#include "cxzerocurve.h"
#include "tcurve.h"
#include "version.h"

static TCurve *discount(TDate today,TDate *dates,double *zeros,int count,double shift,long dcc) {
    double values[64];
    for(int i=0;i<count;i++) values[i]=zeros[i]+shift;
    return JpmcdsMakeTCurve(today,dates,values,count,5000,dcc);
}
int main(int argc,char **argv) {
    if(argc!=4) return 2;
    FILE *nodes=fopen(argv[1],"r"),*cases=fopen(argv[2],"r");
    if(!nodes || !cases) return 3;
    TDate today=JpmcdsDate(2022,6,21),start=today,step=today+1,settle=JpmcdsDate(2022,6,24);
    TDate dates[64],ends[6];double zeros[64],original[6]={0.004,0.006,0.008,0.011,0.014,0.016};
    long dcc,curve_dcc,offset;double df;int count=0;char version[256];
    TDateInterval interval;TStubMethod stub;
    if(JpmcdsStringToDayCountConv("Act/360",&dcc)!=0 || JpmcdsStringToDayCountConv("Act/365F",&curve_dcc)!=0 ||
       JpmcdsStringToDateInterval("3M","credit capture",&interval)!=0 || JpmcdsStringToStubMethod("f/s",&stub)!=0 || JpmcdsVersionString(version)!=0) return 4;
    int years[6]={2023,2024,2025,2027,2029,2032};
    for(int i=0;i<6;i++) ends[i]=JpmcdsDate(years[i],6,20);
    while(count<64 && fscanf(nodes,"%ld %lf",&offset,&df)==2) {
        if(offset==0) {if(df!=1) return 5;continue;}
        if(offset<0 || df<=0 || !isfinite(df)) return 6;
        dates[count]=today+offset;zeros[count]=-log(df)/(offset/365.0);count++;
    }
    if(count!=35) return 7;
    fclose(nodes);
    TCurve *base_discount=discount(today,dates,zeros,count,0,curve_dcc);
    if(!base_discount) return 8;
    TCurve *base_credit=JpmcdsCleanSpreadCurve(today,base_discount,start,step,settle,6,ends,original,NULL,0.4,1,&interval,dcc,&stub,'F',argv[3]);
    if(!base_credit) return 9;
    printf("{\"model_version\":\"");
    for(char *v=version;*v;v++) {if(*v=='"' || *v=='\\') putchar('\\');putchar(*v);}
    printf("\",\"experiments\":[");
    char id[128];int held,experiments=0;double shift,recovery,rates[6];
    while(experiments<64) {
        int got=fscanf(cases,"%127s %d %lf %lf %lf %lf %lf %lf %lf %lf",id,&held,&shift,&recovery,&rates[0],&rates[1],&rates[2],&rates[3],&rates[4],&rates[5]);
        if(got==EOF) break;if(got!=10) return 10;
        TCurve *dc=discount(today,dates,zeros,count,shift,curve_dcc);
        TCurve *cc=held?base_credit:JpmcdsCleanSpreadCurve(today,dc,start,step,settle,6,ends,rates,NULL,recovery,1,&interval,dcc,&stub,'F',argv[3]);
        if(!dc || !cc) return 11;
        if(experiments) printf(",");
        printf("{\"id\":\"%s\",\"survival_probabilities\":[",id);
        for(int i=0;i<6;i++) {if(i) printf(",");printf("%.17g",JpmcdsZeroPrice(cc,ends[i]));}
        printf("],\"hazards_per_act365f_year\":[");
        TDate previous=today;double previous_q=1;
        for(int i=0;i<6;i++) {double q=JpmcdsZeroPrice(cc,ends[i]);if(i) printf(",");printf("%.17g",log(previous_q/q)/((ends[i]-previous)/365.0));previous=ends[i];previous_q=q;}
        printf("],\"valuations\":[");
        for(int i=0;i<6;i++) for(int j=0;j<2;j++) {
            double coupon=j?0.05:0.01,clean,dirty,pv;
            if(JpmcdsCdsPrice(today,settle,step,start,ends[i],coupon,1,&interval,&stub,dcc,'F',argv[3],dc,cc,recovery,1,&clean)!=0 ||
               JpmcdsCdsPrice(today,settle,step,start,ends[i],coupon,1,&interval,&stub,dcc,'F',argv[3],dc,cc,recovery,0,&dirty)!=0 ||
               JpmcdsCdsPrice(today,today,step,start,ends[i],coupon,1,&interval,&stub,dcc,'F',argv[3],dc,cc,recovery,0,&pv)!=0) return 12;
            if(i || j) printf(",");
            printf("{\"pillar_index\":%d,\"coupon_decimal\":%.17g,\"clean_upfront_fraction\":%.17g,\"dirty_upfront_fraction\":%.17g,\"net_pv\":%.17g}",i,coupon,clean,dirty,pv*1e7);
        }
        printf("]}");experiments++;
        if(!held) JpmcdsFreeTCurve(cc);JpmcdsFreeTCurve(dc);
    }
    if(experiments!=23) return 13;
    printf("]}\n");fclose(cases);JpmcdsFreeTCurve(base_credit);JpmcdsFreeTCurve(base_discount);return 0;
}
