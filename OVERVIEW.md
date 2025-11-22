# Convex - Claude Code Setup Overview

```
 ╔═══════════════════════════════════════════════════════════════════╗
 ║                                                                   ║
 ║   ██████╗ ██████╗ ██╗   ██╗██╗   ██╗███████╗██╗  ██╗            ║
 ║  ██╔════╝██╔═══██╗████╗  ██║██║   ██║██╔════╝╚██╗██╔╝            ║
 ║  ██║     ██║   ██║██╔██╗ ██║██║   ██║█████╗   ╚███╔╝             ║
 ║  ██║     ██║   ██║██║╚██╗██║╚██╗ ██╔╝██╔══╝   ██╔██╗             ║
 ║  ╚██████╗╚██████╔╝██║ ╚████║ ╚████╔╝ ███████╗██╔╝ ██╗            ║
 ║   ╚═════╝ ╚═════╝ ╚═╝  ╚═══╝  ╚═══╝  ╚══════╝╚═╝  ╚═╝            ║
 ║                                                                   ║
 ║  High-Performance Fixed Income Analytics Library in Rust         ║
 ║  Complete Claude Code Development Setup                          ║
 ║                                                                   ║
 ╚═══════════════════════════════════════════════════════════════════╝
```

## 📦 What You Have

This package contains **everything needed** to start building Convex with Claude Code:

```
convex-claude-setup.tar.gz (34KB)
│
├── .claude/                          # Claude Code Configuration
│   ├── context.md                    # 📚 Domain Knowledge (Fixed Income)
│   ├── architecture.md               # 🏗️  System Architecture & Diagrams
│   ├── memory.md                     # 🧠 Decisions & Progress Tracking
│   ├── conventions.md                # 📏 Rust Coding Standards
│   ├── prompts.md                    # 💬 Example Prompts Library
│   ├── quickstart.md                 # 🚀 Getting Started Guide
│   ├── checklist.md                  # ✅ Development Checklist
│   └── SETUP_GUIDE.md               # 📖 Complete Setup Instructions
│
├── Cargo.toml.template              # ⚙️  Workspace Configuration
└── README.md.template               # 📝 Project README Template
```

## 🎯 What This Setup Enables

### For Claude Code
- **Deep Domain Understanding**: Comprehensive fixed income analytics knowledge
- **Architectural Guidance**: Clear system design and component interactions
- **Code Quality**: Rust best practices and conventions
- **Consistent Output**: Follows established patterns
- **Self-Documenting**: Claude references files to make informed decisions

### For You
- **Faster Development**: Less explaining, more building
- **Higher Quality**: Built-in best practices
- **Better Architecture**: Well-thought-out design
- **Maintainable Code**: Consistent patterns throughout
- **Production Ready**: Performance targets and validation

## 📊 Project Scope

```
┌─────────────────────────────────────────────────────────────────┐
│                    CONVEX CAPABILITIES                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  📈 YIELD CURVES                   💰 BOND PRICING             │
│  • Bootstrap from market data      • Fixed-rate bonds          │
│  • Multiple interpolation          • Zero-coupon bonds         │
│  • Nelson-Siegel, Svensson        • Callable/putable bonds    │
│  • Multi-curve frameworks         • Floating rate notes       │
│                                                                 │
│  📊 SPREAD ANALYTICS              ⚖️  RISK METRICS             │
│  • Z-Spread                        • Duration (Macaulay, Mod)  │
│  • G-Spread, I-Spread             • Convexity                 │
│  • Asset Swap Spreads             • DV01, Key Rate Durations  │
│  • OAS (Option-Adjusted)          • Greeks (for options)      │
│                                                                 │
│  🌍 MULTI-LANGUAGE                🚀 PERFORMANCE               │
│  • Rust (native)                   • <1μs bond pricing         │
│  • Python (PyO3)                   • <10μs YTM calculation     │
│  • Java (JNI)                      • <100μs curve bootstrap    │
│  • C# (P/Invoke)                   • SIMD optimizations        │
│  • Excel (XLL plugin)              • Parallel processing       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## 🏗️ Architecture at a Glance

```
┌──────────────────────────────────────────────────────────────────┐
│                     CONVEX ARCHITECTURE                          │
└──────────────────────────────────────────────────────────────────┘

    Applications
    ┌──────────────────────────────────────────────────────┐
    │  Python │ Java │ C# │ Excel │ Web API │ Desktop     │
    └────┬─────┴───┬──┴───┬┴───────┴────┬────┴──────────┬──┘
         │         │      │              │               │
         └─────────┴──────┴──────────────┴───────────────┘
                            │
                    Language Bindings
    ┌───────────────────────┴───────────────────────────────┐
    │  PyO3 │ JNI │ P/Invoke │ FFI/C-API │ REST/gRPC      │
    └───────┬───────────────────────────────────────────┬───┘
            │                                           │
            └───────────────────┬───────────────────────┘
                                │
                    Core Rust Library
    ┌───────────────────────────┴──────────────────────────┐
    │                                                       │
    │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐ │
    │  │   Bonds     │  │   Curves    │  │  Spreads    │ │
    │  │   Pricing   │  │   Bootstrap │  │  Analytics  │ │
    │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘ │
    │         │                │                │         │
    │  ┌──────┴────────────────┴────────────────┴──────┐ │
    │  │           Core Infrastructure                 │ │
    │  │  • Types (Date, Price, Yield, Spread)       │ │
    │  │  • Day Count Conventions                     │ │
    │  │  • Business Day Calendars                    │ │
    │  │  • Cash Flow Generation                      │ │
    │  └──────────────────────┬────────────────────────┘ │
    │                         │                           │
    │  ┌──────────────────────┴────────────────────────┐ │
    │  │        Mathematical Engine                    │ │
    │  │  • Root Finders (Newton-Raphson, Brent)     │ │
    │  │  • Optimization (Levenberg-Marquardt)       │ │
    │  │  • Linear Algebra (matrices, solving)       │ │
    │  │  • SIMD Operations                          │ │
    │  └─────────────────────────────────────────────┘ │
    │                                                    │
    └────────────────────────────────────────────────────┘
```

## 🚀 Quick Start (3 Steps)

### Step 1: Extract & Initialize
```bash
# Extract the archive
tar -xzf convex-claude-setup.tar.gz

# Create project directory
mkdir convex && cd convex

# Copy configuration files
cp -r /path/to/.claude .
cp /path/to/Cargo.toml.template Cargo.toml
cp /path/to/README.md.template README.md

# Initialize git
git init && git add . && git commit -m "Initial setup"
```

### Step 2: Start Claude Code
```bash
# Launch Claude Code in project directory
claude code
```

### Step 3: First Prompt
```
I'm starting the Convex fixed income analytics library. Please read:
- .claude/context.md (domain knowledge)
- .claude/architecture.md (system design)
- .claude/conventions.md (coding standards)

Then create the Cargo workspace with all crates following the 
architecture defined in .claude/architecture.md.
```

**That's it!** Claude will set everything up and you can start building.

## 📚 Key Files Explained

### 1. context.md (📚 The Brain)
**What**: Complete fixed income domain knowledge
**Size**: ~15KB of pure gold
**Contains**:
- Bond types, pricing formulas, conventions
- Bloomberg YAS methodology
- Performance targets
- Industry standards

**Why Important**: Claude understands what you're building without constant explanation

### 2. architecture.md (🏗️ The Blueprint)
**What**: Visual system design
**Size**: ~18KB with diagrams
**Contains**:
- Mermaid diagrams of system architecture
- Component relationships
- Data flow diagrams
- Module structure

**Why Important**: Claude designs consistent, well-structured code

### 3. conventions.md (📏 The Standards)
**What**: Rust coding best practices
**Size**: ~12KB of patterns
**Contains**:
- Naming conventions
- Error handling patterns
- Performance optimizations
- Testing standards

**Why Important**: All code follows the same high-quality patterns

### 4. prompts.md (💬 The Templates)
**What**: Example prompts for common tasks
**Size**: ~9KB of examples
**Contains**:
- Feature implementation prompts
- Debugging prompts
- Testing prompts
- Review prompts

**Why Important**: Never struggle with "how do I ask Claude to..."

### 5. memory.md (🧠 The Journal)
**What**: Decision log and progress tracker
**Size**: Grows with project
**Contains**:
- Architectural decisions with rationale
- Implementation progress
- Known issues
- Performance metrics

**Why Important**: Maintains consistency across sessions

## 💡 Development Workflow

```
┌─────────────────────────────────────────────────────────────────┐
│                    TYPICAL WORKFLOW                             │
└─────────────────────────────────────────────────────────────────┘

1. CHECK CHECKLIST
   └── Look at .claude/checklist.md for next task
       
2. PROMPT CLAUDE
   └── Use template from .claude/prompts.md
   └── Reference .claude/context.md sections
       
3. CLAUDE GENERATES
   └── Reads context files
   └── Follows architecture patterns
   └── Applies conventions
   └── Creates code + tests + docs
       
4. REVIEW & TEST
   └── cargo test
   └── cargo clippy
   └── cargo bench
       
5. UPDATE MEMORY
   └── Document decisions in .claude/memory.md
   └── Mark checklist items complete
       
6. COMMIT
   └── git add . && git commit
       
7. REPEAT
   └── Go to step 1 for next feature

         ┌────────────────────────────┐
         │  CONTINUOUS IMPROVEMENT    │
         │  • Refine prompts         │
         │  • Update conventions      │
         │  • Track performance       │
         │  • Document learnings      │
         └────────────────────────────┘
```

## 🎓 Learning Resources

All documentation includes:

✅ **Mathematical Formulas**: LaTeX notation for algorithms  
✅ **Code Examples**: Real-world usage patterns  
✅ **Academic References**: Papers and textbooks  
✅ **Industry Standards**: Bloomberg, ISDA, ICMA  
✅ **Performance Targets**: Measurable goals  
✅ **Validation Criteria**: How to verify correctness  

## 🔥 Performance Targets

```
┌────────────────────────────────────────────────────┐
│  OPERATION              │  TARGET      │  STATUS   │
├─────────────────────────┼──────────────┼──────────┤
│  Bond Pricing           │  < 1 μs      │  🎯      │
│  YTM Calculation        │  < 10 μs     │  🎯      │
│  Curve Bootstrap (50pt) │  < 100 μs    │  🎯      │
│  Z-Spread               │  < 50 μs     │  🎯      │
│  Portfolio (1000 bonds) │  < 10 ms     │  🎯      │
└────────────────────────────────────────────────────┘
```

## 📈 Development Timeline

```
Phase 1: Foundation (Weeks 1-2)
├── Core types (Date, Price, Yield)
├── Day count conventions
└── Business day calendars

Phase 2: Yield Curves (Weeks 3-4)
├── Bootstrap algorithms
├── Interpolation methods
└── Curve validation

Phase 3: Bond Pricing (Weeks 5-6)
├── Fixed-rate bonds
├── YTM calculations
└── Bloomberg validation

Phase 4: Spread Analytics (Weeks 7-8)
├── G-Spread, Z-Spread
├── Asset Swap spreads
└── Performance optimization

Phase 5: Risk Metrics (Weeks 9-10)
├── Duration & Convexity
├── DV01 calculations
└── Key rate durations

Phase 6: Advanced Features (Weeks 11-14)
├── Callable/putable bonds
├── Floating rate notes
└── Multi-curve framework

Phase 7: Language Bindings (Weeks 15-18)
├── Python (PyO3)
├── Java (JNI)
├── C# (P/Invoke)
└── Excel plugin

Phase 8: Production (Weeks 19-24)
├── Performance tuning
├── Market data integration
├── REST API
└── Deployment
```

## 🛠️ Tools & Dependencies

**Required**:
- Rust 1.75+ (latest stable)
- Claude Code CLI

**Key Dependencies**:
- `rust_decimal` - Precise financial math
- `chrono` - Date/time handling
- `rayon` - Parallel processing
- `nalgebra` - Linear algebra
- `criterion` - Benchmarking

**Optional**:
- `cargo-watch` - Auto-recompile
- `cargo-nextest` - Fast testing
- `cargo-flamegraph` - Profiling

## ✨ Key Features of This Setup

1. **📚 Comprehensive Documentation**: Every domain concept explained
2. **🏗️ Visual Architecture**: Mermaid diagrams show relationships
3. **📏 Coding Standards**: Rust best practices built-in
4. **💬 Prompt Library**: Never start from scratch
5. **🧠 Memory System**: Track decisions and progress
6. **✅ Detailed Checklist**: Clear path from start to finish
7. **🚀 Quick Start**: Get productive in minutes
8. **🎯 Performance Goals**: Clear targets to hit

## 🎁 Bonus Materials Included

- Complete Cargo.toml workspace template
- Professional README template
- Example code snippets throughout
- Bloomberg comparison methodology
- Academic references
- Industry standard references

## 📞 Next Steps

1. **Extract the archive**
2. **Read `.claude/SETUP_GUIDE.md`** (comprehensive instructions)
3. **Follow `.claude/quickstart.md`** (step-by-step)
4. **Start with first prompt** (example included)
5. **Use `.claude/prompts.md`** for guidance
6. **Track progress** in `.claude/checklist.md`

## 🎯 Success Criteria

You'll know the setup is working when:

✅ Claude understands fixed income concepts without explanation  
✅ Code follows Rust best practices automatically  
✅ Architecture remains consistent across modules  
✅ Tests achieve >90% coverage  
✅ Performance meets targets (<1μs bond pricing)  
✅ Bloomberg validation passes  

## 🚀 Ready to Build!

Everything you need is in this package. Start with:

```bash
claude code
```

And use the first prompt from the quickstart guide.

**Happy building!** 🦀

---

```
 ╔═══════════════════════════════════════════════════════╗
 ║  Built with ❤️  for quantitative finance developers  ║
 ║  Powered by Claude Code and Rust                     ║
 ╚═══════════════════════════════════════════════════════╝
```
