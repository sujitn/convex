/* Test-only caller of the unmodified ISDA CDS Standard Model 1.8.3.
 * This application is based on the ISDA CDS Standard Model (version 1.8.3),
 * developed and supported in collaboration with Markit Group Ltd.
 * External model sources retain their original notices and public license.
 * This file is a bounded driver, not a production valuation implementation.
 */
#include <stdio.h>
#include <stdlib.h>
#include "dateconv.h"
#include "date_sup.h"
#include "ldate.h"
#include "convert.h"
#include "busday.h"
#include "cdsone.h"
#include "zerocurve.h"
#include "version.h"
#include "cxzerocurve.h"
#include "tcurve.h"

int main(int argc, char **argv) {
    if (argc != 4) return 2;
    FILE *rates_file = fopen(argv[1], "r");
    FILE *cases_file = fopen(argv[2], "r");
    if (!rates_file || !cases_file) return 3;
    char *calendar=argv[3], types[20]={0}, tenor[16], version[256];
    double rates[19]; TDate dates[19];
    TDate today=JpmcdsDate(2022,6,21), spot, settlement, stepin=today+1, start;
    long dcc; TDateInterval interval; TStubMethod stub;
    if (JpmcdsDateFromBusDaysOffset(today,2,calendar,&spot)!=0 ||
        JpmcdsDateFromBusDaysOffset(today,3,calendar,&settlement)!=0 ||
        JpmcdsBusinessDay(JpmcdsDate(2022,6,20),'F',calendar,&start)!=0 ||
        JpmcdsStringToDayCountConv("Act/360",&dcc)!=0 ||
        JpmcdsStringToDateInterval("3M","capture",&interval)!=0 ||
        JpmcdsStringToStubMethod("f/s",&stub)!=0 ||
        JpmcdsVersionString(version)!=0) return 4;
    for (int i=0;i<19;i++) {
        TDateInterval term;
        if(fscanf(rates_file,"%15s %lf",tenor,&rates[i])!=2 ||
           JpmcdsStringToDateInterval(tenor,"capture",&term)!=0 ||
           JpmcdsDateFwdThenAdjust(spot,&term,'N',calendar,&dates[i])!=0) return 5;
        types[i]='S';
    }
    if(fscanf(rates_file,"%15s",tenor)!=EOF) return 6;
    fclose(rates_file);
    TCurve *curve=JpmcdsBuildIRZeroCurve(spot,types,dates,rates,19,dcc,1,1,dcc,dcc,'M',calendar);
    if (!curve) return 7;
    double base=JpmcdsZeroPrice(curve,today);
    printf("{\"model_version\":\"");
    for(char *v=version;*v;v++) { if(*v=='"' || *v=='\\') putchar('\\');putchar(*v); }
    printf("\",\"curve_base_days\":%ld,\"nodes\":[{\"days\":0,\"df\":1},{\"days\":%ld,\"df\":%.17g}",spot-today,spot-today,1/base);
    for(int i=0;i<curve->fNumItems;i++) {
        TDate date=curve->fArray[i].fDate;
        printf(",{\"days\":%ld,\"df\":%.17g}",date-today,JpmcdsZeroPrice(curve,date)/base);
    }
    printf("],\"cases\":[");
    int row,y,m,d,count=0; double coupon,spread,recovery;
    while(count<10000) {
        int scanned=fscanf(cases_file,"%d %d %d %d %lf %lf %lf",&row,&y,&m,&d,&coupon,&spread,&recovery);
        if(scanned==EOF) break;
        if(scanned!=7) return 8;
        TDate maturity=JpmcdsDate(y,m,d); double clean,dirty;
        if(JpmcdsCdsoneUpfrontCharge(today,settlement,start,stepin,start,maturity,coupon,1,&interval,&stub,dcc,'F',calendar,curve,spread,recovery,1,&clean)!=0 ||
           JpmcdsCdsoneUpfrontCharge(today,settlement,start,stepin,start,maturity,coupon,1,&interval,&stub,dcc,'F',calendar,curve,spread,recovery,0,&dirty)!=0) return 9;
        if(count) printf(",");
        printf("{\"row\":%d,\"clean_upfront_fraction\":%.17g,\"dirty_upfront_fraction\":%.17g,\"cash_settlement_amount\":%.17g,\"clean_price_per100\":%.17g,\"accrued_days\":%.17g,\"accrued_premium\":%.17g}",row,clean,dirty,dirty*1e7,(1-clean)*100,(clean-dirty)/coupon*360,(clean-dirty)*1e7);
        count++;
    }
    if(count!=7164) return 10;
    printf("]}\n");
    fclose(cases_file);JpmcdsFreeTCurve(curve);return 0;
}
