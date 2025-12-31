# Convex Analytics Demo - Implementation Plan

## Executive Summary

This plan outlines the implementation of a comprehensive bond pricing and ETF analytics demo application showcasing the Convex library's capabilities. The demo will use **live market data** fetched from public APIs with synthetic fallback.

**Key Principle: Complete Separation** - The demo is a standalone application with its own codebase, dependencies, and deployment. It does NOT modify or depend on the core Convex library source code.

---

## Demo Architecture: Separation from Main Library

### Repository Structure

```
convex/                          # Main repository
├── crates/                      # Rust library (unchanged)
│   ├── convex-bonds/
│   ├── convex-curves/
│   ├── convex-engine/
│   ├── convex-server/           # Optional: REST API server
│   └── ...
├── demo/                        # Demo documentation & data only
│   ├── IMPLEMENTATION_PLAN.md
│   └── data/                    # Static data files (copied to demo app)
│       ├── treasury-curve-live.json
│       ├── corporate-bonds.json
│       └── ...
└── ...

convex-demo/                     # SEPARATE directory (or repo)
├── package.json                 # Demo-specific dependencies
├── src/
│   ├── services/                # API clients, data fetching
│   ├── components/              # React UI components
│   ├── lib/                     # Demo-specific utilities
│   └── ...
├── public/
│   └── data/                    # Static data (copied from convex/demo/data)
└── ...
```

### Integration Options

| Mode | Description | Use Case |
|------|-------------|----------|
| **REST API** | Demo connects to convex-server for calculations | **Full integration demo (PRIMARY)** |
| **WASM** | Demo uses convex-wasm for in-browser calculations | Fallback when server unavailable |
| **MCP** | Demo uses MCP protocol for Claude integration | MCP showcase |
| **Standalone** | Demo runs entirely client-side with simulated data | Offline demo |

### Recommended: REST API (Primary) + WASM (Fallback)

To showcase the **full functionality** of the Convex library, the demo will:
1. **Primary:** Connect to `convex-server` deployed on Fly.io (free tier)
2. **Fallback:** Use `convex-wasm` for client-side calculations if server unavailable
3. **Offline:** Static data with JavaScript calculations as last resort

This architecture demonstrates:
- Real-time WebSocket streaming from convex-server
- Full calculation engine (pricing router, spreads, OAS)
- ETF iNAV calculations
- Portfolio analytics
- Curve bootstrapping

### Full-Stack Demo Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           FRONTEND (Cloudflare Pages)                    │
│                     https://convex-demo.pages.dev                        │
├─────────────────────────────────────────────────────────────────────────┤
│  React App                                                               │
│  ├── REST API calls ──────────────► convex-server (Fly.io)              │
│  │                                   └── /api/v1/price                  │
│  │                                   └── /api/v1/yield                  │
│  │                                   └── /api/v1/spreads                │
│  │                                   └── /api/v1/bootstrap              │
│  │                                                                       │
│  ├── WebSocket ───────────────────► convex-server (Fly.io)              │
│  │                                   └── /ws (real-time quotes)         │
│  │                                                                       │
│  ├── WASM (fallback) ─────────────► convex-wasm (in-browser)            │
│  │                                                                       │
│  └── Static Data ─────────────────► public/data/*.json                  │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                           BACKEND (Fly.io - Free Tier)                   │
│                     https://convex-server.fly.dev                        │
├─────────────────────────────────────────────────────────────────────────┤
│  convex-server (Rust/Axum)                                               │
│  ├── REST Endpoints                                                      │
│  │   ├── POST /api/v1/bonds/price      - Price from yield               │
│  │   ├── POST /api/v1/bonds/yield      - Yield from price               │
│  │   ├── POST /api/v1/bonds/analytics  - Full analytics                 │
│  │   ├── POST /api/v1/spreads/z        - Z-spread                       │
│  │   ├── POST /api/v1/spreads/oas      - OAS calculation                │
│  │   ├── POST /api/v1/curves/bootstrap - Curve bootstrapping            │
│  │   ├── POST /api/v1/etf/inav         - iNAV calculation               │
│  │   └── POST /api/v1/portfolio/risk   - Portfolio analytics            │
│  │                                                                       │
│  └── WebSocket                                                           │
│      └── /ws - Real-time quote streaming                                │
│          ├── subscribe:bond:{id}                                        │
│          ├── subscribe:etf:{ticker}                                     │
│          └── subscribe:curve:{id}                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Data Status (COMPLETED)

### Treasury Yield Curve (Dec 29, 2025 - FRED API)

| Tenor | Rate (%) | Series ID |
|-------|----------|-----------|
| 1M    | 3.69     | DGS1MO    |
| 3M    | 3.68     | DGS3MO    |
| 6M    | 3.59     | DGS6MO    |
| 1Y    | 3.48     | DGS1      |
| 2Y    | 3.45     | DGS2      |
| 3Y    | 3.51     | DGS3      |
| 5Y    | 3.67     | DGS5      |
| 7Y    | 3.88     | DGS7      |
| 10Y   | 4.12     | DGS10     |
| 20Y   | 4.75     | DGS20     |
| 30Y   | 4.80     | DGS30     |

**Note:** Curve is inverted at short end (1M > 2Y), typical late-cycle shape.

### Overnight Rates (Dec 30, 2025)

| Rate | Value (%) | Source |
|------|-----------|--------|
| SOFR | 3.71      | NY Fed |
| Fed Funds | 3.64 | FRED |
| 1M Term SOFR | 4.14 | Pensford |

### SOFR Swap Rates (Dec 30, 2025 - Pensford)

| Tenor | Rate (%) |
|-------|----------|
| 5Y    | 3.36     |
| 10Y   | 3.645    |

**Gap:** 2Y, 3Y, 7Y, 15Y, 20Y, 30Y swap rates need interpolation or synthetic generation.

### Credit Spreads (Dec 29, 2025 - FRED)

| Index | OAS (bps) | Series ID |
|-------|-----------|-----------|
| IG Corporate | 79 | BAMLC0A0CM |
| HY Corporate | 287 | BAMLH0A0HYM2 |

### Volatility (Dec 29, 2025)

| Measure | Value |
|---------|-------|
| MOVE Index | 59.21 |

**Note:** MOVE at 59 is near 52-week lows, indicating low rate volatility environment.

### ETF Prices (Dec 30, 2025)

| ETF | Ticker | Price ($) | NAV ($) | AUM ($B) |
|-----|--------|-----------|---------|----------|
| iShares IG Corporate | LQD | 110.65 | ~110.50 | 32.98 |
| iShares High Yield | HYG | 80.71 | 80.41 | 18.39 |
| iShares 20+ Treasury | TLT | 87.87 | 87.84 | 48.18 |
| iShares Core Aggregate | AGG | 100.16 | ~100.00 | 134.86 |

---

## Phase 2: Data Gaps & Synthetic Generation

### Required Synthetic Data

1. **Full SOFR Swap Curve** - Interpolate missing tenors using:
   - Short end: SOFR + term structure from OIS
   - Mid/long: Linear interpolation between 5Y (3.36%) and 10Y (3.645%)
   - Estimated curve:
     - 2Y: 3.20%
     - 3Y: 3.25%
     - 7Y: 3.50%
     - 15Y: 3.90%
     - 20Y: 4.10%
     - 30Y: 4.25%

2. **ETF Holdings** - Use iShares fact sheets or generate representative portfolios:
   - LQD: ~2,800 IG corporate bonds
   - HYG: ~1,200 HY corporate bonds
   - TLT: ~40 Treasury bonds (20Y+ maturity)
   - AGG: ~12,000 bonds (diverse)

3. **Individual Bond Prices** - For portfolio holdings:
   - Generate synthetic prices from yield + spread
   - Apply realistic bid-ask spreads

4. **Swaption Vol Surface** - Generate SABR-calibrated surface:
   - ATM vols: ~60-80bps (consistent with MOVE at 59)
   - Expiries: 1M, 3M, 6M, 1Y, 2Y, 5Y, 10Y
   - Underlying tenors: 1Y, 2Y, 5Y, 10Y, 30Y

---

## Phase 3: Architecture

### Technology Stack

| Component | Technology | Rationale |
|-----------|------------|-----------|
| Frontend Framework | React 18 + TypeScript | Type safety, modern hooks |
| Build Tool | Vite | Fast HMR, ESM native |
| Styling | Tailwind CSS | Utility-first, consistent |
| UI Components | shadcn/ui + Radix | Accessible, customizable |
| Charts | Recharts | React-native, composable |
| 3D Surface | Three.js + react-three-fiber | Vol surface visualization |
| State Management | Zustand | Simple, performant |
| Data Fetching | TanStack Query | Caching, refetching |
| Real-time | SSE (EventSource) | Simpler than WebSocket |

### Backend Integration

The demo connects to Convex via:
1. **MCP Server** (primary) - stdio transport for local dev
2. **REST API** - HTTP endpoints from convex-server
3. **Mock Mode** - Fallback with realistic calculations

### Data Flow

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│ FRED API    │────▶│ Data Service │────▶│ React Store │
│ NY Fed API  │     │ (cache/SSE)  │     │ (Zustand)   │
└─────────────┘     └──────────────┘     └─────────────┘
                           │                    │
                           ▼                    ▼
                    ┌──────────────┐     ┌─────────────┐
                    │ Convex MCP   │────▶│ UI Components│
                    │ (analytics)  │     │ (React)      │
                    └──────────────┘     └─────────────┘
```

---

## Phase 4: Component Breakdown

### 4.1 Market Monitor Module

```
src/components/MarketMonitor/
├── MarketMonitor.tsx      # Main grid container
├── BondRow.tsx            # Individual bond row with flash
├── PriceCell.tsx          # Colored price with tick indicator
├── SpreadCell.tsx         # Spread display with change
├── SortableHeader.tsx     # Column sorting controls
└── MarketFilters.tsx      # Filter by sector, rating, etc.
```

**Features:**
- Real-time price grid (15-second refresh)
- Color-coded changes (green/red flash)
- Sortable by any column
- Filter by sector, rating, maturity
- Export to CSV

### 4.2 Bond Calculator Module

```
src/components/BondCalculator/
├── BondCalculator.tsx     # Main calculator layout
├── PriceYieldInput.tsx    # Input with solve toggle
├── RiskMetrics.tsx        # Duration, convexity, DV01
├── SpreadAnalysis.tsx     # Z/I/G/ASW spread display
├── CashflowTable.tsx      # Cashflow schedule
└── SensitivityChart.tsx   # Price/yield sensitivity
```

**Features:**
- Price ↔ Yield bidirectional solve
- Real-time risk metrics
- Multiple spread calculations
- Cashflow visualization
- 1bp/10bp sensitivity grid

### 4.3 Yield Curve Module

```
src/components/YieldCurve/
├── YieldCurveChart.tsx    # Main curve visualization
├── CurveOverlay.tsx       # Multi-curve comparison
├── ForwardRates.tsx       # Forward rate toggle
├── CurveShifter.tsx       # Scenario shift controls
├── BootstrapPanel.tsx     # Curve bootstrapping UI
└── CurveTable.tsx         # Tabular rate display
```

**Features:**
- Interactive D3/Recharts curve
- Multiple curve overlay (Treasury, SOFR, Credit)
- Forward rate visualization
- Parallel/twist shift scenarios
- Bootstrap from instruments

### 4.4 ETF Analytics Module

```
src/components/ETF/
├── ETFDashboard.tsx       # Main ETF view
├── INavMonitor.tsx        # Real-time iNAV
├── PremiumDiscount.tsx    # P/D chart
├── HoldingsGrid.tsx       # Full holdings table
├── SectorBreakdown.tsx    # Pie/bar sector view
├── DurationProfile.tsx    # Duration distribution
└── KeyRateDurations.tsx   # KRD bar chart
```

**Features:**
- NAV/iNAV with 15-second updates
- Premium/discount indicator
- Searchable holdings grid
- Sector/rating breakdown charts
- Portfolio-level KRD

### 4.5 Spread Analytics Module

```
src/components/Spreads/
├── SpreadDashboard.tsx    # Spread overview
├── SpreadCalculator.tsx   # Interactive calculator
├── SpreadHistory.tsx      # Time series chart
├── RelativeValue.tsx      # RV analysis
└── SpreadTable.tsx        # Comparable bonds
```

**Features:**
- Z-spread, I-spread, G-spread
- ASW spread
- OAS (with vol input)
- Spread history charts
- Relative value comparison

### 4.6 Volatility Surface Module

```
src/components/Volatility/
├── VolSurface3D.tsx       # Three.js surface
├── VolGrid.tsx            # 2D grid view
├── SABRCalibration.tsx    # SABR params display
├── VolSlice.tsx           # Single expiry/tenor
└── ScenarioShift.tsx      # Vol shift scenarios
```

**Features:**
- Interactive 3D surface plot
- SABR interpolation
- Smile/skew visualization
- Scenario shift analysis

### 4.7 Real-Time Market Data Module (NEW)

```
src/components/RealTime/
├── LiveMarketFeed.tsx     # Main streaming view
├── TickerTape.tsx         # Scrolling price tape
├── PriceFlash.tsx         # Flash animation component
├── VolumeIndicator.tsx    # Trade volume display
├── StreamControls.tsx     # Start/stop/pause controls
├── ConnectionStatus.tsx   # WebSocket/SSE status
├── LatencyMonitor.tsx     # Update latency display
└── MarketHeatmap.tsx      # Sector/rating heatmap
```

**Features:**
- **Live price streaming** with configurable intervals (100ms - 5s)
- Realistic tick simulation using Ornstein-Uhlenbeck process
- Bid/ask spread animation with trade-through indicators
- Volume-weighted average price (VWAP) calculation
- Color-coded heatmap by sector/rating/duration
- Connection status with auto-reconnect
- Latency monitoring and statistics
- Pause/resume/speed controls for demo

**Real-Time Data Flow:**
```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ Price Simulator │────▶│ SSE/WebSocket    │────▶│ React Component │
│ (O-U Process)   │     │ Event Stream     │     │ (useTransition) │
└─────────────────┘     └──────────────────┘     └─────────────────┘
         │                       │
         ▼                       ▼
┌─────────────────┐     ┌──────────────────┐
│ Market Microstr │     │ Update Buffer    │
│ (bid-ask, vol)  │     │ (batch renders)  │
└─────────────────┘     └──────────────────┘
```

### 4.8 Corporate Bond Types Module (NEW)

```
src/components/BondTypes/
├── BondTypeSelector.tsx   # Bond type tabs/cards
├── FixedBondPanel.tsx     # Fixed coupon bond analytics
├── FRNPanel.tsx           # Floating rate note analytics
├── CallableBondPanel.tsx  # Callable bond analytics
├── BondComparison.tsx     # Side-by-side comparison
└── TypeSpecificMetrics.tsx # Type-specific calcs
```

#### 4.8.1 Fixed Rate Corporate Bonds

**Sample Bonds in Demo:**
| Issuer | Coupon | Maturity | Rating | Z-Spread |
|--------|--------|----------|--------|----------|
| Apple (AAPL) | 5.00% | 2030 | AA+ | 32 bps |
| Microsoft (MSFT) | 4.50% | 2032 | AAA | 25 bps |
| JPMorgan (JPM) | 5.25% | 2035 | A- | 95 bps |
| Verizon (VZ) | 6.00% | 2040 | BBB+ | 142 bps |
| Exxon (XOM) | 4.75% | 2028 | AA- | 45 bps |

**Analytics:**
- YTM, YTC, YTW calculations
- Modified/Macaulay duration
- Convexity (positive)
- Z-spread, I-spread, G-spread
- Asset swap spread (ASW)
- Key rate durations

#### 4.8.2 Floating Rate Notes (FRNs)

**Sample FRNs in Demo:**
| Issuer | Spread | Index | Maturity | Discount Margin |
|--------|--------|-------|----------|-----------------|
| Goldman Sachs | +95bp | SOFR | 2027 | 92 bp |
| Bank of America | +110bp | SOFR | 2028 | 105 bp |
| Citigroup | +125bp | SOFR | 2029 | 118 bp |
| US Treasury FRN | 0bp | 13W T-Bill | 2026 | 0 bp |

**FRN-Specific Analytics:**
- Current coupon rate (index + spread)
- Simple margin
- Discount margin (Z-DM)
- Effective duration (near-zero)
- Spread duration
- Reset risk analysis
- Index rate projection

**FRN Pricing Formula:**
```
Price = Σ [CF_i / (1 + (index + DM))^t_i]

Where:
- CF_i = projected cashflow at time t_i
- index = forward rate from curve
- DM = discount margin (solved)
```

#### 4.8.3 Callable Bonds

**Sample Callable Bonds in Demo:**
| Issuer | Coupon | Maturity | First Call | OAS | Option Value |
|--------|--------|----------|------------|-----|--------------|
| AT&T (T) | 5.50% | 2035 | 2030 | 145bp | 13bp |
| CVS Health | 5.25% | 2033 | 2028 | 112bp | 13bp |
| Ford (F) | 6.25% | 2032 | 2027 | 258bp | 27bp |
| Wells Fargo | 5.75% | 2036 | 2031 | 118bp | 17bp |

**Callable-Specific Analytics:**
- Yield to Worst (YTW)
- Yield to Call (YTC) for each call date
- Workout date (date achieving YTW)
- Option-Adjusted Spread (OAS)
- Z-spread (ignoring optionality)
- Option value = Z-spread - OAS
- Effective duration (shorter than modified)
- Effective convexity (negative near call)
- Make-whole spread analysis

**OAS Calculation Method:**
```
1. Build interest rate tree (Hull-White)
2. Calibrate to swaption vol surface
3. Backward induction with call decision:
   Value[node] = min(CallPrice, ContinuationValue)
4. Binary search for spread making model price = market price
```

---

## Phase 5: File Structure

### Main Convex Repository (unchanged)
```
convex/                              # Main repository - NOT MODIFIED
├── crates/                          # Rust library crates
│   ├── convex-bonds/
│   ├── convex-curves/
│   ├── convex-engine/
│   ├── convex-server/
│   ├── convex-wasm/                 # WASM package (used by demo)
│   └── ...
├── demo/                            # Documentation & static data ONLY
│   ├── IMPLEMENTATION_PLAN.md       # This file
│   └── data/                        # Reference data (copied to demo app)
│       ├── treasury-curve-live.json
│       ├── sofr-rates-live.json
│       ├── corporate-bonds.json
│       └── ...
└── ...
```

### Standalone Demo Application (SEPARATE REPO)
```
convex-demo/                         # Separate repository
├── .github/
│   └── workflows/
│       ├── ci.yml                   # Lint, test, build
│       └── deploy.yml               # Deploy to Pages
├── public/
│   ├── data/                        # Static data (copied from convex/demo/data)
│   │   ├── treasury-curve.json
│   │   ├── sofr-rates.json
│   │   ├── credit-spreads.json
│   │   ├── swap-rates.json
│   │   ├── corporate-bonds.json
│   │   ├── etf-prices.json
│   │   └── volatility.json
│   └── wasm/                        # WASM files (from convex-wasm build)
│       ├── convex_wasm_bg.wasm
│       └── convex_wasm.js
├── src/
│   ├── config/
│   │   ├── environment.ts           # Environment config
│   │   ├── api-endpoints.ts         # API endpoints
│   │   └── constants.ts             # App constants
│   ├── services/
│   │   ├── convex-wasm.ts           # WASM wrapper (primary calc engine)
│   │   ├── fred-client.ts           # FRED API client
│   │   ├── nyfed-client.ts          # NY Fed API client
│   │   ├── market-data.ts           # Market data orchestration
│   │   ├── static-data.ts           # Static JSON loader
│   │   ├── price-simulator.ts       # Real-time price simulation
│   │   ├── realtime-stream.ts       # SSE event stream
│   │   └── bond-universe.ts         # Bond data management
│   ├── stores/
│   │   ├── market-store.ts          # Market data state
│   │   ├── curve-store.ts           # Curve state
│   │   ├── bond-store.ts            # Bond universe state
│   │   ├── etf-store.ts             # ETF analytics state
│   │   ├── realtime-store.ts        # Real-time stream state
│   │   └── settings-store.ts        # User preferences
│   ├── hooks/
│   │   ├── useMarketData.ts         # Market data hook
│   │   ├── useBondPricing.ts        # Bond pricing hook
│   │   ├── useCurves.ts             # Curve data hook
│   │   ├── useETFAnalytics.ts       # ETF analytics hook
│   │   ├── useRealTimeStream.ts     # Real-time stream hook
│   │   ├── useFRNAnalytics.ts       # FRN-specific analytics
│   │   └── useCallableAnalytics.ts  # Callable bond analytics
│   ├── components/
│   │   ├── layout/
│   │   │   ├── AppShell.tsx         # Main app layout
│   │   │   ├── Sidebar.tsx          # Navigation sidebar
│   │   │   ├── Header.tsx           # Top header
│   │   │   └── Footer.tsx           # Footer with data sources
│   │   ├── ui/                      # shadcn/ui components
│   │   │   ├── button.tsx
│   │   │   ├── card.tsx
│   │   │   ├── tabs.tsx
│   │   │   └── ...
│   │   ├── MarketMonitor/
│   │   │   ├── MarketMonitor.tsx
│   │   │   ├── BondRow.tsx
│   │   │   ├── PriceCell.tsx
│   │   │   └── MarketFilters.tsx
│   │   ├── BondCalculator/
│   │   │   ├── BondCalculator.tsx
│   │   │   ├── PriceYieldInput.tsx
│   │   │   ├── RiskMetrics.tsx
│   │   │   └── CashflowTable.tsx
│   │   ├── YieldCurve/
│   │   │   ├── YieldCurveChart.tsx
│   │   │   ├── CurveOverlay.tsx
│   │   │   └── BootstrapPanel.tsx
│   │   ├── ETF/
│   │   │   ├── ETFDashboard.tsx
│   │   │   ├── INavMonitor.tsx
│   │   │   └── HoldingsGrid.tsx
│   │   ├── Spreads/
│   │   │   ├── SpreadDashboard.tsx
│   │   │   └── SpreadCalculator.tsx
│   │   ├── Volatility/
│   │   │   ├── VolSurface3D.tsx
│   │   │   └── VolGrid.tsx
│   │   ├── RealTime/
│   │   │   ├── LiveMarketFeed.tsx
│   │   │   ├── TickerTape.tsx
│   │   │   ├── PriceFlash.tsx
│   │   │   ├── MarketHeatmap.tsx
│   │   │   └── StreamControls.tsx
│   │   ├── BondTypes/
│   │   │   ├── BondTypeSelector.tsx
│   │   │   ├── FixedBondPanel.tsx
│   │   │   ├── FRNPanel.tsx
│   │   │   ├── CallableBondPanel.tsx
│   │   │   └── BondComparison.tsx
│   │   └── demos/
│   │       ├── DemoRunner.tsx
│   │       └── ScenarioCard.tsx
│   ├── lib/
│   │   ├── analytics/               # Fallback JS analytics (if WASM unavailable)
│   │   │   ├── fixed-bond.ts
│   │   │   ├── frn.ts
│   │   │   ├── callable.ts
│   │   │   └── oas-calculator.ts
│   │   ├── simulation/
│   │   │   ├── ou-process.ts        # Ornstein-Uhlenbeck for yields
│   │   │   └── market-micro.ts      # Bid-ask spread simulation
│   │   ├── formatters.ts            # Number/date formatters
│   │   └── utils.ts                 # General utilities
│   ├── scenarios/
│   │   ├── index.ts                 # All scenario exports
│   │   ├── treasury-pricing.ts
│   │   ├── corporate-spreads.ts
│   │   ├── curve-bootstrap.ts
│   │   ├── etf-inav.ts
│   │   ├── rate-shock.ts
│   │   ├── callable-oas.ts
│   │   ├── realtime-feed.ts
│   │   ├── fixed-corporate.ts
│   │   ├── frn-analysis.ts
│   │   ├── callable-deep-dive.ts
│   │   └── bond-comparison.ts
│   ├── App.tsx
│   ├── main.tsx
│   └── index.css                    # Tailwind imports
├── .env.example                     # Environment template
├── .gitignore
├── package.json
├── package-lock.json
├── tailwind.config.js
├── postcss.config.js
├── vite.config.ts
├── tsconfig.json
├── tsconfig.node.json
├── lighthouserc.json                # Lighthouse CI config
└── README.md
```

### Data Flow Between Repos

```
convex/                          convex-demo/
┌────────────────────┐           ┌────────────────────┐
│ crates/convex-wasm │──build──▶ │ public/wasm/       │
│ (Rust → WASM)      │           │ convex_wasm.wasm   │
└────────────────────┘           └────────────────────┘

┌────────────────────┐           ┌────────────────────┐
│ demo/data/         │──copy───▶ │ public/data/       │
│ *.json (reference) │           │ *.json (runtime)   │
└────────────────────┘           └────────────────────┘
```

---

## Phase 6: Implementation Order

### Sprint 1: Foundation (4-6 hours)

1. **Project Setup**
   - Create Vite project with TypeScript
   - Install dependencies
   - Configure Tailwind + shadcn/ui
   - Set up folder structure

2. **Data Layer**
   - FRED API client
   - NY Fed API client
   - Market data service with caching
   - Price simulator for real-time updates

3. **State Management**
   - Zustand stores
   - React Query setup
   - SSE integration for updates

### Sprint 2: Core UI (4-6 hours)

4. **Layout & Navigation**
   - App shell with sidebar
   - Module navigation
   - Responsive design

5. **Market Monitor**
   - Bond grid with real-time updates
   - Sorting and filtering
   - Price change flash animation

6. **Yield Curve Visualizer**
   - Treasury curve chart
   - Multi-curve overlay
   - Forward rate toggle

### Sprint 3: Analytics (4-6 hours)

7. **Bond Calculator**
   - Price/yield solver
   - Risk metrics display
   - Cashflow table

8. **ETF Analytics**
   - NAV/iNAV monitor
   - Holdings grid
   - Duration profile

9. **Spread Analytics**
   - Z-spread calculator
   - Spread comparison
   - History charts

### Sprint 4: Advanced (4-6 hours)

10. **MCP Integration**
    - MCP client wrapper
    - Fallback to mock mode
    - Error handling

11. **Volatility Surface**
    - 3D surface with Three.js
    - SABR calibration display

12. **Demo Scenarios**
    - Interactive walkthroughs
    - Guided demos

### Sprint 5: Polish (2-4 hours)

13. **Testing & Validation**
    - Calculation validation
    - Cross-browser testing

14. **Performance**
    - Bundle optimization
    - Lazy loading

15. **Documentation**
    - README
    - Demo script

---

## Phase 7: Demo Scenarios

### Scenario 1: Price a 10Y Treasury
1. Show current 10Y rate (4.12%)
2. Input price of 98.50
3. Calculate YTM, duration, DV01
4. Show sensitivity to 1bp move

### Scenario 2: Corporate Spread Analysis
1. Select Apple 10Y bond
2. Show Z-spread vs Treasury curve
3. Compare I-spread, G-spread
4. Decompose into components

### Scenario 3: Bootstrap SOFR Curve
1. Input current swap rates
2. Run bootstrap algorithm
3. Visualize zero curve
4. Show forward rates

### Scenario 4: ETF iNAV Tracking
1. Select LQD ETF
2. Show real-time iNAV updates
3. Compare to market price
4. Highlight premium/discount

### Scenario 5: Rate Shock Stress Test
1. Load sample portfolio
2. Apply +100bp parallel shift
3. Show P&L impact
4. Display duration contribution

### Scenario 6: Callable Bond OAS
1. Select callable corporate bond
2. Input vol assumption (MOVE-based)
3. Calculate OAS vs Z-spread
4. Show option value

### Scenario 7: Real-Time Market Feed (NEW)
1. Start live price stream for 15 corporate bonds
2. Watch prices tick with bid/ask animation
3. Show sector heatmap updating in real-time
4. Demonstrate latency monitoring
5. Pause/resume stream to examine specific tick
6. Show VWAP calculation updating
7. Highlight price anomalies (wide spreads, fast moves)

**Technical Details:**
- Ornstein-Uhlenbeck mean-reverting process for yield simulation
- Configurable tick interval: 100ms (fast), 500ms (normal), 2s (slow)
- Realistic bid-ask spreads by asset class:
  - Treasuries: 0.5/32 (~1.5 cents)
  - IG Corporate: 12.5 cents
  - HY Corporate: 37.5 cents
- Volume simulation based on market cap/liquidity

### Scenario 8: Fixed Rate Corporate Bond Pricing (NEW)
1. Select Microsoft 4.5% 2032 bond
2. Show current market price: $99.81
3. Calculate analytics:
   - YTM: 4.535%
   - Modified Duration: 5.62
   - Convexity: 0.38
   - DV01: $0.0562
4. Calculate spreads:
   - Z-spread: 25 bps
   - I-spread: 21 bps (vs SOFR swap)
   - G-spread: 28 bps (vs Treasury)
5. Generate cashflow table
6. Show price sensitivity grid

### Scenario 9: Floating Rate Note Analysis (NEW)
1. Select Goldman Sachs SOFR+95bp 2027 FRN
2. Show current index rate: SOFR = 3.71%
3. Calculate current coupon: 4.66%
4. Calculate FRN-specific metrics:
   - Discount Margin: 92 bps
   - Simple Margin: 91 bps
   - Effective Duration: 0.22 years (near-zero)
   - Spread Duration: 1.45 years
5. Project forward coupons using SOFR curve
6. Compare to fixed-rate alternative
7. Show sensitivity to spread changes

**FRN Key Points to Demonstrate:**
- Near-zero rate duration (resets to par at each coupon)
- Spread duration = credit risk exposure
- DM vs Simple Margin difference
- Index rate projection from forward curve

### Scenario 10: Callable Bond Deep Dive (NEW)
1. Select AT&T 5.5% 2035 callable (first call 2030)
2. Display call schedule:
   - 2030: 102.75
   - 2031: 101.833
   - 2032: 100.917
   - 2033: 100.00
3. Calculate yield measures:
   - YTM: 5.665%
   - YTC (2030): 5.42%
   - **YTW: 5.42%** (workout at first call)
4. Calculate spreads:
   - Z-spread: 158 bps
   - **OAS: 145 bps**
   - Option value: 13 bps
5. Show effective duration (4.25) vs modified (7.65)
6. Explain negative convexity near call strike
7. Demonstrate vol sensitivity (higher vol → lower OAS)

**Callable Bond Key Points:**
- YTW is the relevant yield measure
- OAS removes optionality; Z-spread includes it
- Option value widens as rates fall (call more likely)
- Effective duration < modified duration when callable
- Negative convexity = price appreciation capped

### Scenario 11: Bond Type Comparison (NEW)
1. Compare 3 bonds with similar credit/maturity:
   - Fixed: JPM 5.25% 2035 (A-)
   - FRN: BAC SOFR+110 2028 (A-)
   - Callable: WFC 5.75% 2036 callable 2031 (A)
2. Side-by-side metrics table
3. Show duration profile differences
4. Scenario: +50bp rate shock
   - Fixed: -3.5% price impact
   - FRN: -0.1% price impact
   - Callable: -2.1% price impact
5. Scenario: +25bp spread shock
   - All three: similar impact based on spread duration
6. Explain when each type is preferred:
   - Fixed: stable rate view, want duration
   - FRN: rising rate hedge, credit exposure only
   - Callable: yield pickup, accept call risk

---

## Phase 8: Acceptance Criteria

### Functional Requirements

- [ ] Treasury curve displays live data from FRED
- [ ] SOFR rate updates from NY Fed
- [ ] ETF prices refresh every 15 seconds
- [ ] Yield calculations are accurate within 0.5bp
- [ ] Z-spread solver converges for all bonds
- [ ] iNAV updates in real-time
- [ ] All 11 demo scenarios run successfully

### Real-Time Data Requirements (NEW)

- [ ] Price stream runs at configurable intervals (100ms-5s)
- [ ] Bid/ask spread animation renders smoothly
- [ ] Heatmap updates without flicker
- [ ] Connection status shows correctly
- [ ] Pause/resume works without data loss
- [ ] Latency statistics update in real-time

### Corporate Bond Type Requirements (NEW)

- [ ] Fixed bonds: YTM, duration, convexity, all spreads calculate correctly
- [ ] FRNs: Discount margin solver converges
- [ ] FRNs: Forward coupon projection works
- [ ] Callable: YTW identifies correct workout date
- [ ] Callable: OAS calculation within 1bp of reference
- [ ] Callable: Effective duration shows negative convexity
- [ ] Bond comparison shows side-by-side analytics

### Non-Functional Requirements

- [ ] Initial load < 3 seconds
- [ ] Price update latency < 100ms
- [ ] Calculation response < 50ms
- [ ] Smooth scrolling at 60fps
- [ ] Mobile-responsive layout
- [ ] Graceful API failure handling

---

## Data Sources Summary

| Data | Primary Source | Fallback |
|------|----------------|----------|
| Treasury Curve | FRED API | Static JSON |
| SOFR | NY Fed API | FRED |
| Swap Rates | Web scrape | Synthetic |
| Credit Spreads | FRED | Static |
| ETF Prices | Yahoo Finance | Web search |
| ETF Holdings | iShares | Synthetic |
| Volatility | MOVE Index | Static |

---

## Phase 9: Deployment Plan

### Hosting Options Comparison

#### Frontend (Static)

| Platform | Free Tier | Best For | Limitations | Build Time |
|----------|-----------|----------|-------------|------------|
| **Cloudflare Pages** | Unlimited | Performance, global CDN | 500 builds/month | ~1 min |
| **Vercel** | 100GB/month | React/Next.js, previews | 100 deploys/day | ~30 sec |
| **Netlify** | 100GB/month | Static sites, forms | 300 build min/month | ~1 min |
| **GitHub Pages** | Unlimited | Static sites, open source | No SSR, public repos only | ~2 min |

#### Backend (Rust Server)

| Platform | Free Tier | Best For | Limitations | Cold Start |
|----------|-----------|----------|-------------|------------|
| **Fly.io** | 3 shared VMs | Rust/Go, WebSocket | 256MB RAM free | ~2-3s |
| **Railway** | $5 credit/month | Docker, databases | Credit runs out | ~3s |
| **Render** | 750 hrs/month | Web services | Spins down after 15min | ~30s |
| **Shuttle** | 3 projects | Rust-native | Beta, limited | ~5s |
| **Koyeb** | 1 nano instance | Docker, global | 256MB RAM | ~2s |

### Recommended Stack

| Component | Platform | URL | Cost |
|-----------|----------|-----|------|
| **Frontend** | Cloudflare Pages | `https://convex-demo.pages.dev` | Free |
| **Backend** | Fly.io | `https://convex-server.fly.dev` | Free (3 VMs) |
| **Backup Frontend** | GitHub Pages | `https://sujitn.github.io/convex-demo` | Free |

### Why This Stack?

**Fly.io for Backend:**
- Free tier includes 3 shared-cpu-1x VMs (256MB each)
- Supports WebSocket connections (critical for real-time)
- Global edge deployment (low latency)
- Rust binary runs efficiently in small containers
- No cold start if kept alive with health checks
- Free outbound data transfer

**Cloudflare Pages for Frontend:**
- Unlimited bandwidth and requests
- Global CDN (fastest static hosting)
- Preview deployments for PRs
- Direct GitHub integration

---

## Phase 10: Backend Deployment (convex-server)

### Fly.io Setup

#### Step 1: Install Fly CLI and Login

```bash
# Install flyctl
curl -L https://fly.io/install.sh | sh

# Or on Windows
powershell -Command "iwr https://fly.io/install.ps1 -useb | iex"

# Login (creates free account)
fly auth login
```

#### Step 2: Create Dockerfile for convex-server

```dockerfile
# Dockerfile in convex/crates/convex-server/
FROM rust:1.75-slim as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy workspace
COPY . .

# Build release binary
RUN cargo build --release -p convex-server

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/convex-server /usr/local/bin/

# Copy static data
COPY demo/data /app/data

ENV RUST_LOG=info
ENV DATA_DIR=/app/data
EXPOSE 8080

CMD ["convex-server", "--host", "0.0.0.0", "--port", "8080"]
```

#### Step 3: Create fly.toml

```toml
# fly.toml
app = "convex-server"
primary_region = "iad"  # US East (or choose your region)

[build]
  dockerfile = "crates/convex-server/Dockerfile"

[env]
  RUST_LOG = "info"
  DATA_DIR = "/app/data"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = false  # Keep alive for WebSocket
  auto_start_machines = true
  min_machines_running = 1

  [http_service.concurrency]
    type = "connections"
    hard_limit = 100
    soft_limit = 80

[[vm]]
  cpu_kind = "shared"
  cpus = 1
  memory_mb = 256

# Health check to prevent cold starts
[[services.http_checks]]
  interval = "30s"
  timeout = "5s"
  path = "/health"
```

#### Step 4: Deploy

```bash
# From convex repo root
cd /path/to/convex

# Launch app (first time)
fly launch --dockerfile crates/convex-server/Dockerfile

# Deploy updates
fly deploy

# View logs
fly logs

# Check status
fly status
```

#### Step 5: Configure CORS for Demo

Update convex-server to allow CORS from demo domain:

```rust
// In convex-server/src/main.rs or routes.rs
use tower_http::cors::{CorsLayer, Any};

let cors = CorsLayer::new()
    .allow_origin([
        "https://convex-demo.pages.dev".parse().unwrap(),
        "https://sujitn.github.io".parse().unwrap(),
        "http://localhost:5173".parse().unwrap(),  // Local dev
    ])
    .allow_methods(Any)
    .allow_headers(Any);

let app = Router::new()
    // ... routes
    .layer(cors);
```

### Alternative: Railway Deployment

```bash
# Install Railway CLI
npm i -g @railway/cli

# Login
railway login

# Initialize project
railway init

# Deploy
railway up
```

```toml
# railway.toml
[build]
builder = "dockerfile"
dockerfilePath = "crates/convex-server/Dockerfile"

[deploy]
startCommand = "convex-server --host 0.0.0.0 --port $PORT"
healthcheckPath = "/health"
healthcheckTimeout = 100
```

### Keep-Alive Strategy (Prevent Cold Starts)

Since free tiers spin down after inactivity, implement keep-alive:

```typescript
// In demo frontend: src/services/server-keepalive.ts
const KEEPALIVE_INTERVAL = 5 * 60 * 1000; // 5 minutes

export function startKeepAlive(serverUrl: string) {
  setInterval(async () => {
    try {
      await fetch(`${serverUrl}/health`);
    } catch (e) {
      console.warn('Server keepalive failed:', e);
    }
  }, KEEPALIVE_INTERVAL);
}
```

### Server API Contract

```typescript
// src/services/convex-api.ts
const API_BASE = import.meta.env.VITE_CONVEX_SERVER_URL || 'https://convex-server.fly.dev';

export interface BondPriceRequest {
  coupon: number;
  maturityDate: string;
  yield: number;
  settlementDate?: string;
  frequency?: number;
  dayCount?: string;
}

export interface BondAnalyticsResponse {
  price: number;
  yield: number;
  modifiedDuration: number;
  macaulayDuration: number;
  convexity: number;
  dv01: number;
  accruedInterest: number;
}

export interface SpreadRequest {
  bondId: string;
  price: number;
  curveId: string;
}

export interface SpreadResponse {
  zSpread: number;
  iSpread: number;
  gSpread: number;
  asw: number;
  oas?: number;
}

export class ConvexAPI {
  private baseUrl: string;

  constructor(baseUrl = API_BASE) {
    this.baseUrl = baseUrl;
  }

  async calculatePrice(request: BondPriceRequest): Promise<number> {
    const response = await fetch(`${this.baseUrl}/api/v1/bonds/price`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
    const data = await response.json();
    return data.price;
  }

  async calculateYield(coupon: number, maturity: string, price: number): Promise<number> {
    const response = await fetch(`${this.baseUrl}/api/v1/bonds/yield`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ coupon, maturityDate: maturity, price }),
    });
    const data = await response.json();
    return data.yield;
  }

  async getFullAnalytics(bondId: string, price: number): Promise<BondAnalyticsResponse> {
    const response = await fetch(`${this.baseUrl}/api/v1/bonds/analytics`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ bondId, price }),
    });
    return response.json();
  }

  async calculateSpreads(request: SpreadRequest): Promise<SpreadResponse> {
    const response = await fetch(`${this.baseUrl}/api/v1/spreads`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
    return response.json();
  }

  // WebSocket connection for real-time quotes
  connectWebSocket(onMessage: (data: any) => void): WebSocket {
    const ws = new WebSocket(`${this.baseUrl.replace('https', 'wss')}/ws`);
    ws.onmessage = (event) => onMessage(JSON.parse(event.data));
    return ws;
  }
}
```

---

## Phase 11: Frontend Deployment

### Recommended: Cloudflare Pages (Primary) + GitHub Pages (Backup)

**Why Cloudflare Pages:**
- Unlimited free bandwidth and requests
- Global CDN with edge caching (fastest)
- Automatic HTTPS
- Preview deployments for PRs
- No build minute limits that matter
- Direct GitHub integration
- Custom domains free

**Why GitHub Pages as backup:**
- Zero configuration for GitHub repos
- Built-in to GitHub Actions
- Good enough performance
- Completely free forever

### Deployment Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     convex-demo Repository                       │
├─────────────────────────────────────────────────────────────────┤
│  main branch                                                     │
│  └── push triggers ──┬──► Cloudflare Pages (production)         │
│                      │    └── https://convex-demo.pages.dev     │
│                      │                                          │
│                      └──► GitHub Pages (backup)                 │
│                           └── https://sujitn.github.io/convex-demo │
│                                                                 │
│  PR branches                                                    │
│  └── push triggers ──────► Cloudflare Preview                   │
│                            └── https://<hash>.convex-demo.pages.dev │
└─────────────────────────────────────────────────────────────────┘
```

### Setup Instructions

#### Option 1: Cloudflare Pages (Recommended)

```bash
# 1. Create GitHub repository
gh repo create convex-demo --public --clone
cd convex-demo

# 2. Initialize project
npm create vite@latest . -- --template react-ts
npm install

# 3. Build configuration (vite.config.ts already correct for SPA)

# 4. Connect to Cloudflare Pages
# - Go to https://dash.cloudflare.com/
# - Pages → Create a project → Connect to Git
# - Select convex-demo repository
# - Build settings:
#   - Build command: npm run build
#   - Build output directory: dist
#   - Root directory: (leave empty)

# 5. Custom domain (optional)
# - Pages → convex-demo → Custom domains → Add
# - demo.convex.dev (if you own convex.dev)
```

#### Option 2: GitHub Pages

```yaml
# .github/workflows/deploy.yml
name: Deploy to GitHub Pages

on:
  push:
    branches: [main]

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'

      - run: npm ci
      - run: npm run build

      - uses: actions/upload-pages-artifact@v3
        with:
          path: dist

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - uses: actions/deploy-pages@v4
        id: deployment
```

```typescript
// vite.config.ts - for GitHub Pages
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: '/convex-demo/', // Repository name for GitHub Pages
})
```

#### Option 3: Vercel (One-Click)

```bash
# Using Vercel CLI
npm i -g vercel
vercel

# Or connect via dashboard:
# - Go to https://vercel.com/new
# - Import convex-demo from GitHub
# - Framework: Vite
# - Build: npm run build
# - Output: dist
```

### Environment Configuration

```typescript
// src/config/environment.ts
export const config = {
  // API endpoints - demo uses static data by default
  apiMode: import.meta.env.VITE_API_MODE || 'static', // 'static' | 'rest' | 'wasm'

  // FRED API (for live data refresh)
  fredApiKey: import.meta.env.VITE_FRED_API_KEY || '0e506a03f80d59fb3f77b710850c2638',

  // Optional: Convex server for REST mode
  convexServerUrl: import.meta.env.VITE_CONVEX_SERVER_URL || 'http://localhost:3000',

  // Feature flags
  enableRealTimeStream: import.meta.env.VITE_ENABLE_REALTIME !== 'false',
  enableWasm: import.meta.env.VITE_ENABLE_WASM !== 'false',
};
```

```bash
# .env.example (for local development)
VITE_API_MODE=static
VITE_FRED_API_KEY=0e506a03f80d59fb3f77b710850c2638
VITE_ENABLE_REALTIME=true
VITE_ENABLE_WASM=true
```

### WASM Integration for Calculations

```typescript
// src/services/convex-wasm.ts
import init, {
  calculate_yield,
  calculate_price,
  calculate_duration,
  calculate_z_spread,
  bootstrap_curve,
} from 'convex-wasm';

let wasmInitialized = false;

export async function initConvexWasm(): Promise<void> {
  if (!wasmInitialized) {
    await init();
    wasmInitialized = true;
  }
}

export async function calculateYield(
  coupon: number,
  maturityYears: number,
  price: number,
  frequency: number = 2
): Promise<number> {
  await initConvexWasm();
  return calculate_yield(coupon, maturityYears, price, frequency);
}

// ... other wasm wrappers
```

### Build & Bundle Size Optimization

```typescript
// vite.config.ts - Production optimizations
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { visualizer } from 'rollup-plugin-visualizer'

export default defineConfig({
  plugins: [
    react(),
    visualizer({ open: false, filename: 'dist/stats.html' }),
  ],
  build: {
    target: 'esnext',
    minify: 'esbuild',
    rollupOptions: {
      output: {
        manualChunks: {
          'vendor-react': ['react', 'react-dom'],
          'vendor-charts': ['recharts', 'd3'],
          'vendor-ui': ['@radix-ui/react-dialog', '@radix-ui/react-tabs'],
          'convex-wasm': ['convex-wasm'],
        },
      },
    },
  },
  optimizeDeps: {
    exclude: ['convex-wasm'], // WASM loaded separately
  },
})
```

### Expected Bundle Sizes

| Chunk | Size (gzip) | Contents |
|-------|-------------|----------|
| vendor-react | ~45 KB | React, ReactDOM |
| vendor-charts | ~85 KB | Recharts, D3 |
| vendor-ui | ~25 KB | Radix UI components |
| convex-wasm | ~150 KB | Bond calculations |
| app | ~60 KB | Demo application code |
| **Total** | **~365 KB** | First load JS |

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| First Contentful Paint | < 1.5s | Lighthouse |
| Largest Contentful Paint | < 2.5s | Lighthouse |
| Time to Interactive | < 3.5s | Lighthouse |
| Total Blocking Time | < 200ms | Lighthouse |
| Cumulative Layout Shift | < 0.1 | Lighthouse |

### CI/CD Pipeline

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

jobs:
  lint-and-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'

      - run: npm ci
      - run: npm run lint
      - run: npm run type-check
      - run: npm run test
      - run: npm run build

      # Bundle size check
      - name: Check bundle size
        run: |
          BUNDLE_SIZE=$(du -sb dist | cut -f1)
          if [ $BUNDLE_SIZE -gt 2000000 ]; then
            echo "Bundle size exceeds 2MB: $BUNDLE_SIZE bytes"
            exit 1
          fi

  lighthouse:
    runs-on: ubuntu-latest
    needs: lint-and-test
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'

      - run: npm ci
      - run: npm run build

      - name: Lighthouse CI
        uses: treosh/lighthouse-ci-action@v10
        with:
          configPath: ./lighthouserc.json
          uploadArtifacts: true
```

### Deployment Checklist

- [ ] Repository created and pushed
- [ ] Cloudflare Pages connected
- [ ] Custom domain configured (optional)
- [ ] Environment variables set
- [ ] GitHub Actions workflow added
- [ ] WASM package published to npm (or bundled)
- [ ] Lighthouse scores verified
- [ ] Mobile responsiveness tested
- [ ] API fallback verified (works without network)

---

## Confirmation Required

Please confirm:

1. **Scope:** Is the component breakdown appropriate?
2. **Technology:** Any preference changes (charts, UI framework)?
3. **Priority:** Which modules are most critical for the demo?
4. **Integration:** Should we prioritize MCP integration or focus on standalone demo first?
5. **Timeline:** Any constraints on delivery?

Once confirmed, I will begin Sprint 1 implementation.
