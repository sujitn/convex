# Bloomberg Field Mapping Reference

## Duration Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `MOD_DUR` | Modified Duration | Years | % price change per 100bp yield change |
| `MACD` | Macaulay Duration | Years | Weighted average time to cash flows |
| `OAD` | Option-Adjusted Duration | Years | Effective duration for optioned bonds |
| `EFF_DUR` | Effective Duration | Years | Alternative name for OAD |
| `DUR_MID` | Duration (Mid) | Years | Duration at mid price |
| `DUR_ADJ_MID` | Duration Adjusted (Mid) | Years | Adjusted for settlement |

## DV01/Risk Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `RISK` | DV01/Risk | $ per 1bp | Bloomberg's primary DV01 measure |
| `DV01` | Dollar Value of 01 | $ per 1bp | Price change per 1bp yield change |
| `PV01` | Present Value of 01 | $ per 1bp | Often used interchangeably with DV01 |
| `BPV` | Basis Point Value | $ per 1bp | Same as DV01 |

### Risk Calculation
```
Bloomberg RISK = MOD_DUR × Price / 100 (per 100bp)
DV01 = RISK / 100 (per 1bp)
```

## Convexity Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `CONVEXITY` | Convexity | Years² | Second derivative measure |
| `CNVX_MID` | Convexity (Mid) | Years² | At mid price |
| `OAC` | Option-Adjusted Convexity | Years² | For optioned bonds |
| `EFF_CNVX` | Effective Convexity | Years² | Alternative name for OAC |

### Scaling Note
Bloomberg may report convexity scaled by 100. Verify with:
```
ΔP/P ≈ -Duration × Δy + 0.5 × (Convexity/100) × Δy²
```

## Spread Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `OAS_SPREAD_MID` | Option-Adjusted Spread | bps | Spread over Treasury/swap |
| `Z_SPRD_MID` | Z-Spread | bps | Spread over spot curve |
| `I_SPRD_MID` | I-Spread | bps | Spread over interpolated swap |
| `G_SPRD_MID` | G-Spread | bps | Spread over govt interpolated |
| `ASW_SPREAD` | Asset Swap Spread | bps | Par-par asset swap spread |
| `ASSET_SWAP_SPD_MID` | ASW (Mid) | bps | Asset swap at mid |
| `DM` | Discount Margin | bps | For FRNs |
| `DIS_MRG_MID` | Discount Margin (Mid) | bps | DM at mid price |

## Spread Duration Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `OAS_DUR` | OAS Duration | Years | OAS spread sensitivity |
| `OASD` | OAS Spread Duration | Years | Alias for OAS_DUR |
| `SPD_DUR` | Spread Duration | Years | General spread duration |
| `CRD_DUR` | Credit Duration | Years | Credit spread sensitivity |
| `CS01` | Credit Spread 01 | $ per 1bp | Dollar credit spread sensitivity |

## Yield Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `YLD_YTM_MID` | Yield to Maturity | % | Yield at mid price |
| `YLD_CNV_MID` | Yield to Convention | % | Street convention yield |
| `YLD_YTC_MID` | Yield to Call | % | For callable bonds |
| `YLD_YTP_MID` | Yield to Put | % | For putable bonds |
| `YLD_YTW_MID` | Yield to Worst | % | Minimum of YTM/YTC/YTP |

## Price Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `PX_MID` | Mid Price | % of par | Clean price |
| `PX_BID` | Bid Price | % of par | Clean bid |
| `PX_ASK` | Ask Price | % of par | Clean ask |
| `PX_DIRTY_MID` | Dirty Mid | % of par | Full/invoice price |
| `INT_ACC` | Accrued Interest | % of par | Accrued since last coupon |

## YAS Function Output Fields

The Bloomberg YAS (Yield Analysis) function provides these key outputs:

### Main Analytics Tab
| Field | Description |
|-------|-------------|
| Price | Clean price |
| Yield | Yield to maturity |
| Modified Duration | MOD_DUR |
| Macaulay Duration | MACD |
| Convexity | CONVEXITY |
| Risk | DV01 measure |

### Spread Tab
| Field | Description |
|-------|-------------|
| Z-Spread | Spread over zero curve |
| I-Spread | Spread over swap |
| G-Spread | Spread over govt |
| ASW Spread | Asset swap spread |
| OAS | Option-adjusted (if applicable) |

### Risk Tab
| Field | Description |
|-------|-------------|
| Total Risk | Aggregate DV01 |
| Key Rate Risks | KRD by tenor bucket |
| Spread Risk | Spread DV01 |

## Key Rate Duration Fields

Bloomberg provides KRD via CRVF or YAS Risk tab:

| Tenor | Field Pattern |
|-------|---------------|
| 3M | `KRD_3M` |
| 6M | `KRD_6M` |
| 1Y | `KRD_1Y` |
| 2Y | `KRD_2Y` |
| 3Y | `KRD_3Y` |
| 5Y | `KRD_5Y` |
| 7Y | `KRD_7Y` |
| 10Y | `KRD_10Y` |
| 20Y | `KRD_20Y` |
| 30Y | `KRD_30Y` |

## Floating Rate Note Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `RESET_DT` | Next Reset Date | Date | When coupon resets |
| `CPN_TYP` | Coupon Type | Code | FLOAT, FIXED, etc. |
| `FLT_SPREAD` | Floating Spread | bps | Quoted margin |
| `DM` | Discount Margin | bps | Required spread |
| `FLT_CPN` | Current Floating Coupon | % | Index + spread |
| `RESET_IDX` | Reset Index | Code | SOFR, EURIBOR, etc. |

## Inflation-Linked Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `REAL_YLD` | Real Yield | % | Inflation-adjusted yield |
| `BEI` | Breakeven Inflation | % | Implied inflation |
| `INDEX_RATIO` | Index Ratio | Number | Inflation adjustment factor |
| `INFLATION_DUR` | Inflation Duration | Years | BEI sensitivity |
| `REAL_DUR` | Real Duration | Years | Real yield sensitivity |

## Callable/Putable Fields

| Bloomberg Field | Description | Units | Notes |
|-----------------|-------------|-------|-------|
| `NXT_CALL_DT` | Next Call Date | Date | Earliest call |
| `NXT_CALL_PX` | Next Call Price | % of par | Call strike |
| `MAKE_WHOLE` | Make-Whole Call | Boolean | Treasury + spread call |
| `OAS_VOL` | OAS Volatility | % | Rate vol used in OAS |
| `VEGA` | Vega | $/vol point | Volatility sensitivity |

## Day Count Convention Codes

| Bloomberg Code | Convention |
|----------------|------------|
| `ACT/ACT` | Actual/Actual ICMA |
| `ACT/360` | Actual/360 |
| `ACT/365` | Actual/365 Fixed |
| `30/360` | 30/360 US |
| `30E/360` | 30E/360 (European) |
| `30E+/360` | 30E+/360 ISDA |

## Business Day Convention Codes

| Bloomberg Code | Convention |
|----------------|------------|
| `F` | Following |
| `MF` | Modified Following |
| `P` | Preceding |
| `MP` | Modified Preceding |
| `U` | Unadjusted |

## Settlement Convention Fields

| Bloomberg Field | Description |
|-----------------|-------------|
| `SETTLE_DT` | Settlement Date |
| `DAYS_TO_SETTLE` | T+N settlement |
| `PREV_CPN_DT` | Previous Coupon Date |
| `NXT_CPN_DT` | Next Coupon Date |
| `FIRST_CPN_DT` | First Coupon Date |

## Currency-Specific Defaults

| Market | Day Count | Compounding | Settlement |
|--------|-----------|-------------|------------|
| US Treasury | ACT/ACT | Semi-annual | T+1 |
| US Corporate | 30/360 | Semi-annual | T+2 |
| UK Gilts | ACT/ACT | Semi-annual | T+1 |
| EUR Govt | ACT/ACT | Annual | T+2 |
| EUR Corporate | ACT/ACT | Annual | T+2 |
| JPY Govt | ACT/365 | Semi-annual | T+2 |
