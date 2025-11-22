# Convex Architecture

## System Architecture Overview

```mermaid
graph TB
    subgraph "User Applications"
        Python[Python Application]
        Java[Java Application]
        CSharp[C# Application]
        Excel[Excel Plugin]
        REST[REST API Client]
    end

    subgraph "Language Bindings Layer"
        PyBinding[convex-python<br/>PyO3]
        JNIBinding[Java JNI]
        PInvoke[C# P/Invoke]
        CAPI[C API<br/>convex-ffi]
    end

    subgraph "Core Rust Library - Convex"
        subgraph "High-Level APIs"
            BondAPI[Bond Pricing API]
            CurveAPI[Curve Building API]
            SpreadAPI[Spread Analytics API]
            RiskAPI[Risk Calculation API]
        end

        subgraph "Domain Layer"
            Bonds[convex-bonds<br/>Bond Instruments]
            Curves[convex-curves<br/>Yield Curves]
            Spreads[convex-spreads<br/>Spread Calculations]
            Risk[Risk Metrics]
        end

        subgraph "Core Infrastructure"
            Types[convex-core<br/>Domain Types]
            DayCount[Day Count<br/>Conventions]
            Calendar[Business Day<br/>Calendars]
            Cashflow[Cash Flow<br/>Engine]
        end

        subgraph "Mathematical Engine"
            Math[convex-math]
            Solvers[Root Finders<br/>Newton-Raphson, Brent]
            Optimization[Optimizers<br/>Levenberg-Marquardt]
            LinAlg[Linear Algebra<br/>Matrix Operations]
            SIMD[SIMD Operations]
        end
    end

    subgraph "Performance & Utilities"
        Cache[Result Cache]
        Parallel[Rayon Parallel<br/>Processing]
        Decimal[High-Precision<br/>Decimal Math]
    end

    %% Connections
    Python --> PyBinding
    Java --> JNIBinding
    CSharp --> PInvoke
    Excel --> CAPI
    REST --> CAPI

    PyBinding --> CAPI
    JNIBinding --> CAPI
    PInvoke --> CAPI

    CAPI --> BondAPI
    CAPI --> CurveAPI
    CAPI --> SpreadAPI
    CAPI --> RiskAPI

    BondAPI --> Bonds
    CurveAPI --> Curves
    SpreadAPI --> Spreads
    RiskAPI --> Risk

    Bonds --> Types
    Bonds --> Cashflow
    Curves --> Types
    Curves --> Math
    Spreads --> Bonds
    Spreads --> Curves
    Risk --> Bonds

    Cashflow --> DayCount
    Cashflow --> Calendar
    Types --> DayCount

    Math --> Solvers
    Math --> Optimization
    Math --> LinAlg
    Math --> SIMD

    Bonds --> Cache
    Curves --> Parallel
    Math --> Decimal

    style BondAPI fill:#4CAF50
    style CurveAPI fill:#4CAF50
    style SpreadAPI fill:#4CAF50
    style RiskAPI fill:#4CAF50
    style Math fill:#2196F3
    style CAPI fill:#FF9800
```

## Component Architecture

### 1. Core Type System (convex-core)

```mermaid
classDiagram
    class Date {
        +year: i32
        +month: u8
        +day: u8
        +add_days(i32) Date
        +add_months(i32) Date
        +is_business_day(Calendar) bool
        +adjust(Convention, Calendar) Date
    }

    class Price {
        +value: Decimal
        +currency: Currency
        +as_percentage() f64
    }

    class Yield {
        +value: Decimal
        +convention: YieldConvention
        +compounding: Compounding
        +frequency: Frequency
    }

    class Spread {
        +value_bps: Decimal
        +spread_type: SpreadType
    }

    class DayCounter {
        <<trait>>
        +day_count_fraction(Date, Date) Decimal
        +year_fraction(Date, Date) Decimal
    }

    class Calendar {
        <<trait>>
        +is_business_day(Date) bool
        +adjust(Date, Convention) Date
        +add_business_days(Date, i32) Date
    }

    DayCounter <|.. Act360
    DayCounter <|.. Act365
    DayCounter <|.. Thirty360
    DayCounter <|.. ActActISDA

    Calendar <|.. USCalendar
    Calendar <|.. UKCalendar
    Calendar <|.. EUCalendar
```

### 2. Bond Pricing Engine

```mermaid
flowchart TD
    Start[Bond Pricing Request] --> LoadBond[Load Bond Specifications]
    LoadBond --> GetCurve[Get Discount Curve]
    GetCurve --> GenCF[Generate Cash Flows]
    
    GenCF --> Loop{For Each<br/>Cash Flow}
    Loop --> CalcDF[Calculate Discount Factor]
    CalcDF --> DiscountCF[Discount Cash Flow]
    DiscountCF --> Loop
    
    Loop --> SumPV[Sum Present Values]
    SumPV --> CalcAccrued[Calculate Accrued Interest]
    CalcAccrued --> CleanPrice[Clean Price = PV - Accrued]
    
    CleanPrice --> Cache{Cache<br/>Result?}
    Cache -->|Yes| Store[Store in Cache]
    Cache -->|No| Return[Return Price]
    Store --> Return
    
    style Start fill:#e1f5ff
    style Return fill:#c8e6c9
    style GenCF fill:#fff9c4
    style SumPV fill:#fff9c4
```

### 3. Yield Curve Construction Flow

```mermaid
flowchart LR
    subgraph Input
        Quotes[Market Quotes<br/>Deposits, Futures, Swaps]
        Config[Curve Configuration<br/>Interpolation Method<br/>Day Count Convention]
    end

    subgraph Bootstrap
        Sort[Sort by Maturity]
        Init[Initialize Zero Rates]
        
        Iterate{For Each<br/>Instrument}
        Solve[Solve for Zero Rate<br/>Newton-Raphson]
        Update[Update Curve Points]
    end

    subgraph Interpolation
        Linear[Linear on Zero Rates]
        Spline[Cubic Spline]
        NS[Nelson-Siegel]
        Svensson[Svensson]
    end

    subgraph Output
        ZeroCurve[Zero Coupon Curve]
        DiscountCurve[Discount Factor Curve]
        ForwardCurve[Forward Rate Curve]
    end

    Quotes --> Sort
    Config --> Sort
    Sort --> Init
    Init --> Iterate
    Iterate --> Solve
    Solve --> Update
    Update --> Iterate
    Iterate --> Linear
    Iterate --> Spline
    Iterate --> NS
    Iterate --> Svensson
    
    Linear --> ZeroCurve
    Spline --> ZeroCurve
    NS --> ZeroCurve
    Svensson --> ZeroCurve
    
    ZeroCurve --> DiscountCurve
    ZeroCurve --> ForwardCurve
```

### 4. Spread Calculation Architecture

```mermaid
graph TD
    Bond[Bond with Market Price] --> Type{Spread Type}
    
    Type -->|Z-Spread| ZCalc[Z-Spread Calculator]
    Type -->|G-Spread| GCalc[G-Spread Calculator]
    Type -->|ASW| ASWCalc[Asset Swap Calculator]
    Type -->|OAS| OASCalc[OAS Calculator]
    
    ZCalc --> ZIterative[Iterative Solver<br/>Find spread that matches price]
    GCalc --> GInterp[Interpolate Government Yield<br/>Subtract from Bond Yield]
    ASWCalc --> ASWPar[Par-Par ASW Calculation]
    ASWCalc --> ASWProceeds[Proceeds ASW Calculation]
    OASCalc --> OASTree[Binomial/Trinomial Tree<br/>With Option Exercise]
    
    ZIterative --> DiscountCurve[Discount Curve]
    GInterp --> GovCurve[Government Curve]
    ASWPar --> SwapCurve[Swap Curve]
    ASWProceeds --> SwapCurve
    OASTree --> VolSurface[Volatility Surface]
    
    ZIterative --> Result[Spread Result]
    GInterp --> Result
    ASWPar --> Result
    ASWProceeds --> Result
    OASTree --> Result
    
    style Type fill:#ffeb3b
    style Result fill:#4caf50
```

### 5. Risk Calculation Pipeline

```mermaid
flowchart TB
    subgraph Inputs
        B[Bond Specification]
        C[Yield Curve]
        P[Market Price]
    end

    subgraph Duration
        D1[Calculate Modified Duration<br/>Analytical Formula]
        D2[Calculate Macaulay Duration<br/>Weighted Average Maturity]
        D3[Calculate Effective Duration<br/>Bump Up/Down Yield]
    end

    subgraph Convexity
        CV1[Calculate Convexity<br/>Second Derivative]
        CV2[Effective Convexity<br/>Finite Differences]
    end

    subgraph DV01
        DV1[Calculate DV01<br/>Price Δ for 1bp yield Δ]
        KR[Key Rate Durations<br/>Per Curve Point]
    end

    subgraph Greeks
        Delta[Option Delta]
        Gamma[Option Gamma]
        Vega[Option Vega]
        Theta[Option Theta]
    end

    B --> D1
    B --> D2
    C --> D1
    C --> D2
    P --> D3
    C --> D3

    D1 --> CV1
    D3 --> CV2
    
    D1 --> DV1
    C --> KR
    
    B --> Delta
    B --> Gamma
    C --> Delta
    
    DV1 --> Report[Risk Report]
    KR --> Report
    CV1 --> Report
    Delta --> Report
    Gamma --> Report
    
    style Report fill:#4caf50
```

## Data Flow Architecture

### Pricing Data Flow

```mermaid
sequenceDiagram
    participant Client
    participant BondAPI
    participant PricingEngine
    participant CurveManager
    participant Cache
    participant Calculator

    Client->>BondAPI: price_bond(bond_id, date)
    BondAPI->>Cache: check_cache(bond_id, date)
    
    alt Cache Hit
        Cache-->>BondAPI: cached_price
        BondAPI-->>Client: price
    else Cache Miss
        BondAPI->>CurveManager: get_curve(date, currency)
        CurveManager-->>BondAPI: yield_curve
        BondAPI->>PricingEngine: calculate_price(bond, curve)
        PricingEngine->>Calculator: generate_cashflows(bond)
        Calculator-->>PricingEngine: cashflow_schedule
        PricingEngine->>Calculator: discount_cashflows(cashflows, curve)
        Calculator-->>PricingEngine: present_value
        PricingEngine-->>BondAPI: price
        BondAPI->>Cache: store(bond_id, date, price)
        BondAPI-->>Client: price
    end
```

## Module Dependency Graph

```mermaid
graph TD
    Core[convex-core] --> Curves[convex-curves]
    Core --> Bonds[convex-bonds]
    Core --> Math[convex-math]
    
    Math --> Curves
    Math --> Bonds
    Math --> Spreads[convex-spreads]
    
    Curves --> Bonds
    Curves --> Spreads
    
    Bonds --> Spreads
    
    Spreads --> API[High-Level API]
    Bonds --> API
    Curves --> API
    
    API --> FFI[convex-ffi]
    FFI --> Python[convex-python]
    FFI --> Java[Java Bindings]
    FFI --> CSharp[C# Bindings]
    
    style Core fill:#2196F3
    style Math fill:#2196F3
    style API fill:#4CAF50
    style FFI fill:#FF9800
```

## Performance Optimization Strategy

```mermaid
mindmap
    root((Performance<br/>Optimization))
        Algorithmic
            Fast Root Finding
                Newton-Raphson
                Brent Method
            Efficient Interpolation
                Binary Search
                Cached Coefficients
            Vectorized Operations
                SIMD Instructions
                Batch Processing
        
        Data Structure
            Cache-Friendly Layouts
                Array of Structs
                Struct of Arrays
            Zero-Copy Operations
                Cow Types
                Reference Passing
            Memory Pooling
                Object Reuse
                Pre-allocation
        
        Parallelization
            Multi-threading
                Rayon Data Parallel
                Work Stealing
            Async Processing
                Tokio Runtime
                Concurrent Curves
        
        Compilation
            LTO
                Link Time Optimization
            PGO
                Profile Guided
            Target Features
                Native CPU
                AVX2/AVX512
```

## Testing Strategy

```mermaid
graph TB
    subgraph "Unit Tests"
        UT1[Component Tests<br/>90% Coverage]
        UT2[Property-Based Tests<br/>Invariant Checking]
    end

    subgraph "Integration Tests"
        IT1[End-to-End Scenarios]
        IT2[Multi-Component Tests]
    end

    subgraph "Validation Tests"
        VT1[Bloomberg Comparison]
        VT2[Reuters Comparison]
        VT3[Known Test Cases]
    end

    subgraph "Performance Tests"
        PT1[Benchmark Suite]
        PT2[Regression Detection]
        PT3[Profiling]
    end

    UT1 --> CI[Continuous Integration]
    UT2 --> CI
    IT1 --> CI
    IT2 --> CI
    VT1 --> Validation[Validation Gate]
    VT2 --> Validation
    VT3 --> Validation
    PT1 --> PerfGate[Performance Gate]
    PT2 --> PerfGate
    
    CI --> Validation
    Validation --> PerfGate
    PerfGate --> Release[Release]
    
    style Release fill:#4caf50
```

## Deployment Architecture

```mermaid
graph LR
    subgraph "Development"
        Dev[Local Development]
        Test[Unit/Integration Tests]
    end

    subgraph "Build Pipeline"
        Compile[Rust Compilation<br/>Multiple Targets]
        Bindings[Generate Bindings<br/>Python/Java/C#]
        Package[Package Artifacts]
    end

    subgraph "Distribution"
        CratesIO[crates.io]
        PyPI[PyPI]
        Maven[Maven Central]
        NuGet[NuGet]
    end

    subgraph "Deployment"
        Lib[Native Library]
        Container[Docker Container]
        Lambda[Serverless Function]
    end

    Dev --> Test
    Test --> Compile
    Compile --> Bindings
    Bindings --> Package
    
    Package --> CratesIO
    Package --> PyPI
    Package --> Maven
    Package --> NuGet
    
    CratesIO --> Lib
    PyPI --> Container
    Maven --> Container
    Package --> Lambda
    
    style Package fill:#4caf50
```

## Security & Safety Architecture

```mermaid
graph TD
    Input[External Input] --> Validation{Input<br/>Validation}
    
    Validation -->|Invalid| Error[Return Error]
    Validation -->|Valid| Sanitize[Sanitize Data]
    
    Sanitize --> TypeCheck[Type-Level Safety<br/>Rust Type System]
    TypeCheck --> BoundCheck[Bounds Checking]
    BoundCheck --> NumCheck[Numerical Stability<br/>Overflow/Underflow]
    
    NumCheck --> Process[Process in Safe Code]
    Process --> Unsafe{Requires<br/>Unsafe?}
    
    Unsafe -->|No| SafePath[Safe Implementation]
    Unsafe -->|Yes| UnsafeDoc[Documented Unsafe Block<br/>Safety Invariants]
    
    UnsafeDoc --> Audit[Security Audit]
    SafePath --> Result[Return Result]
    Audit --> Result
    
    Error --> Log[Log Error]
    Result --> Log
    
    style Error fill:#f44336
    style Result fill:#4caf50
    style Unsafe fill:#ff9800
```
