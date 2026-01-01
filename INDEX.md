# Convex Claude Code Setup - File Index

## Quick Reference Guide

This index provides a quick overview of every file in this setup package.

---

## 📁 Directory Structure

```
convex-claude-setup/
│
├── .claude/                         # Claude Code configuration directory
│   ├── SETUP_GUIDE.md              # 📖 START HERE - Complete setup instructions
│   ├── context.md                  # 📚 Domain knowledge (fixed income)
│   ├── architecture.md             # 🏗️  System architecture with diagrams
│   ├── memory.md                   # 🧠 Decision log and progress tracker
│   ├── conventions.md              # 📏 Rust coding standards
│   ├── prompts.md                  # 💬 Example prompts library
│   ├── quickstart.md               # 🚀 Quick start guide
│   └── checklist.md                # ✅ Development checklist
│
├── Cargo.toml.template             # ⚙️  Workspace configuration template
├── README.md.template              # 📝 Project README template
├── OVERVIEW.md                     # 🎯 Visual project overview
└── INDEX.md                        # 📇 This file
```

---

## 📋 File Descriptions

### Core Configuration Files

#### `.claude/SETUP_GUIDE.md` (START HERE!)
**Purpose**: Master guide for the entire setup  
**Size**: ~15KB  
**Read First**: Yes  
**Update Frequency**: Rarely  

**What's Inside**:
- Overview of all files and their purposes
- Step-by-step setup instructions
- Development workflow patterns
- Best practices for working with Claude Code
- Troubleshooting guide
- Maintenance procedures

**When to Use**: 
- First time setting up the project
- When you're confused about how files work together
- When onboarding new team members
- Reference for workflow patterns

---

#### `.claude/context.md` (THE BRAIN)
**Purpose**: Complete domain knowledge for fixed income analytics  
**Size**: ~60KB  
**Read First**: Claude reads it automatically  
**Update Frequency**: When domain requirements change  

**What's Inside**:
- Fixed income fundamentals (bond types, pricing, yields)
- Complete analytics requirements (YTM, spreads, risk metrics)
- Technical architecture principles
- Day count conventions (exact specifications)
- Industry-standard yield methodology (exact specification)
- Performance targets (<1μs pricing, <10μs YTM)
- Project structure and organization
- Key dependencies and their purposes
- References (academic papers, industry standards)

**When to Reference**:
- When Claude needs domain context
- When implementing new features
- When validating calculations
- When writing documentation

**Example Prompts**:
```
"Implement YTM calculation as described in .claude/context.md section 'Yield Calculation Methodology'"

"Follow the yield methodology in .claude/context.md for sequential roll-forward"

"Use the day count conventions specified in .claude/context.md"
```

---

#### `.claude/architecture.md` (THE BLUEPRINT)
**Purpose**: Visual system architecture and design patterns  
**Size**: ~40KB  
**Read First**: Claude reads it automatically  
**Update Frequency**: When architecture evolves  

**What's Inside**:
- System architecture diagram (Mermaid)
- Component architecture with class diagrams
- Bond pricing engine flow
- Yield curve construction flow
- Spread calculation architecture
- Risk calculation pipeline
- Data flow sequences
- Module dependency graph
- Performance optimization strategy
- Testing strategy
- Deployment architecture
- Security & safety architecture

**When to Reference**:
- When designing new components
- When understanding system interactions
- When reviewing code structure
- When making architectural decisions

**Example Prompts**:
```
"Follow the bond pricing flow diagram in .claude/architecture.md"

"Implement according to the component architecture in .claude/architecture.md"

"Use the module dependency structure from .claude/architecture.md"
```

---

#### `.claude/memory.md` (THE JOURNAL)
**Purpose**: Track decisions, progress, and learnings  
**Size**: Grows with project (starts at ~15KB)  
**Read First**: Optional  
**Update Frequency**: Continuously (after each decision)  

**What's Inside**:
- Architectural decisions log (with rationale)
- Technical decisions (dependencies, frameworks)
- Domain decisions (feature priorities)
- Implementation progress by milestone
- Known issues and challenges
- Performance metrics tracking
- Questions and discussions
- Future enhancements queue
- Testing coverage status
- Dependencies management log

**When to Update**:
- After making architectural decisions
- When completing milestones
- When discovering issues
- When answering open questions
- When performance metrics change

**Example Updates**:
```
"Update .claude/memory.md with decision to use Decimal type for precision"

"Document in .claude/memory.md that we completed Phase 1 milestone"

"Add to .claude/memory.md: Known issue with negative yields in edge cases"
```

---

#### `.claude/conventions.md` (THE STANDARDS)
**Purpose**: Rust coding standards and best practices  
**Size**: ~25KB  
**Read First**: Claude reads it automatically  
**Update Frequency**: When standards evolve  

**What's Inside**:
- Code formatting rules (rustfmt settings)
- Naming conventions (types, functions, constants)
- Type safety patterns (newtypes, builders)
- Error handling standards (thiserror usage)
- Documentation standards (rustdoc with examples)
- Performance best practices (inline, SIMD, iterators)
- Testing conventions (unit, integration, property tests)
- Concurrency patterns (Rayon, Arc, RwLock)
- Module organization principles
- Trait design guidelines
- FFI safety patterns
- Code review checklist

**When to Reference**:
- When Claude isn't following standards
- When reviewing code
- When writing new features
- When setting up CI checks

**Example Prompts**:
```
"Follow the naming conventions in .claude/conventions.md"

"Use the error handling patterns from .claude/conventions.md"

"Apply performance optimizations from .claude/conventions.md"
```

---

#### `.claude/prompts.md` (THE TEMPLATES)
**Purpose**: Example prompts for common development tasks  
**Size**: ~20KB  
**Read First**: When you need prompt ideas  
**Update Frequency**: Add new patterns as discovered  

**What's Inside**:
- Getting started prompts
- Core types development prompts
- Domain-specific prompts (curves, bonds, spreads)
- Testing requirement prompts
- Performance optimization prompts
- Documentation enhancement prompts
- Advanced feature prompts (callable bonds, FFI)
- Debugging and troubleshooting prompts
- Code review prompts
- Maintenance and evolution prompts
- Tips for effective prompts

**When to Use**:
- When starting a new task
- When you're unsure how to phrase a request
- When implementing common patterns
- As templates to customize

**Example Usage**:
```
Copy a prompt template from .claude/prompts.md and customize:

"Implement the ACT/360 day count convention in convex-core with:
- DayCounter trait implementation
- Comprehensive unit tests
- Documentation with examples"
```

---

#### `.claude/quickstart.md` (THE JUMPSTART)
**Purpose**: Step-by-step getting started guide  
**Size**: ~12KB  
**Read First**: After SETUP_GUIDE.md  
**Update Frequency**: When workflow changes  

**What's Inside**:
- Prerequisites (Rust, Claude Code)
- Project setup steps
- First Claude Code session
- Development workflow examples
- Best practices for Claude Code
- Common development tasks
- Testing strategy
- Troubleshooting common issues
- Project structure reference
- Next steps after setup

**When to Use**:
- First time setting up
- Teaching others the workflow
- Quick reference for commands
- When you forgot the workflow

---

#### `.claude/checklist.md` (THE ROADMAP)
**Purpose**: Detailed development checklist by phase  
**Size**: ~18KB  
**Read First**: After initial setup  
**Update Frequency**: Daily (mark completed items)  

**What's Inside**:
- Phase 1: Foundation (Weeks 1-2)
- Phase 2: Yield Curves (Weeks 3-4)
- Phase 3: Bond Pricing (Weeks 5-6)
- Phase 4: Spread Analytics (Weeks 7-8)
- Phase 5: Risk Calculations (Weeks 9-10)
- Phase 6: Advanced Features (Weeks 11-14)
- Phase 7: Language Bindings (Weeks 15-18)
- Phase 8: Production Features (Weeks 19-24)
- Ongoing tasks
- Success criteria

**When to Use**:
- Planning daily/weekly work
- Tracking overall progress
- Ensuring nothing is missed
- Coordinating team work

**How to Update**:
```
Mark completed items:
- [x] Date struct implemented
- [x] ACT/360 day count convention
- [ ] ACT/365 day count convention (in progress)
```

---

### Template Files

#### `Cargo.toml.template`
**Purpose**: Workspace configuration template  
**Size**: ~3KB  
**Usage**: Copy to `Cargo.toml` in project root  
**Customize**: Update author, repository URLs  

**What's Inside**:
- Workspace member definitions
- Shared dependencies configuration
- Build profiles (dev, release, bench, dist)
- Optimization settings (LTO, codegen-units)
- Platform-specific configurations
- Metadata for docs.rs

**When to Use**: Initial project setup

---

#### `README.md.template`
**Purpose**: Professional project README  
**Size**: ~8KB  
**Usage**: Copy to `README.md` in project root  
**Customize**: Update URLs, contact info, examples  

**What's Inside**:
- Project description and features
- Quick start examples (pricing, spreads)
- Architecture overview
- Performance benchmarks table
- Day count conventions list
- Language bindings examples
- Supported bond types
- Development instructions
- Contributing guidelines
- Roadmap by quarter
- License information

**When to Use**: Initial project setup, GitHub README

---

### Supplementary Files

#### `OVERVIEW.md`
**Purpose**: Visual project overview with ASCII art  
**Size**: ~12KB  
**Read First**: For high-level understanding  
**Update Frequency**: Rarely  

**What's Inside**:
- ASCII art logo
- Package contents visualization
- Capabilities overview
- Architecture diagram (ASCII)
- Quick start (3 steps)
- Key files explained
- Development workflow visualization
- Performance targets table
- Timeline diagram
- Key features summary

**When to Use**:
- Getting inspired
- Understanding big picture
- Presentations
- Marketing materials

---

#### `INDEX.md`
**Purpose**: This file - quick reference to all files  
**Size**: ~8KB  
**Read First**: When navigating the setup  
**Update Frequency**: When files are added/changed  

**What's Inside**: You're reading it!

---

## 📖 Reading Order

### For First-Time Setup:
1. `OVERVIEW.md` - Get the big picture (5 min)
2. `.claude/SETUP_GUIDE.md` - Understand the setup (15 min)
3. `.claude/quickstart.md` - Follow step-by-step (10 min)
4. Start coding with Claude!

### For Development:
1. `.claude/checklist.md` - What's next?
2. `.claude/prompts.md` - How to ask Claude?
3. `.claude/context.md` - What's the requirement?
4. `.claude/conventions.md` - How to code it?
5. `.claude/memory.md` - What did we decide?

### For Code Review:
1. `.claude/conventions.md` - Check standards
2. `.claude/architecture.md` - Check structure
3. `.claude/context.md` - Check requirements

### For Troubleshooting:
1. `.claude/quickstart.md` - Common issues section
2. `.claude/SETUP_GUIDE.md` - Troubleshooting section
3. `.claude/memory.md` - Known issues

---

## 🔄 Update Guidelines

### When to Update Each File:

**Never Update** (static templates):
- `Cargo.toml.template`
- `README.md.template`
- `OVERVIEW.md`
- `INDEX.md`

**Rarely Update** (major changes only):
- `.claude/SETUP_GUIDE.md` (workflow changes)
- `.claude/context.md` (new domain knowledge)
- `.claude/architecture.md` (architecture changes)
- `.claude/conventions.md` (new standards)
- `.claude/quickstart.md` (setup process changes)

**Regularly Update**:
- `.claude/memory.md` (after each decision)
- `.claude/checklist.md` (daily progress)

**Continuously Expand**:
- `.claude/prompts.md` (add successful prompts)

---

## 🎯 File Size Reference

```
File                          Size      Importance
────────────────────────────────────────────────────
.claude/context.md            ~60KB     ⭐⭐⭐⭐⭐
.claude/architecture.md       ~40KB     ⭐⭐⭐⭐⭐
.claude/conventions.md        ~25KB     ⭐⭐⭐⭐⭐
.claude/prompts.md           ~20KB     ⭐⭐⭐⭐
.claude/checklist.md         ~18KB     ⭐⭐⭐⭐
.claude/SETUP_GUIDE.md       ~15KB     ⭐⭐⭐⭐⭐
.claude/memory.md            ~15KB+    ⭐⭐⭐⭐
.claude/quickstart.md        ~12KB     ⭐⭐⭐⭐
OVERVIEW.md                  ~12KB     ⭐⭐⭐
README.md.template           ~8KB      ⭐⭐⭐
INDEX.md                     ~8KB      ⭐⭐
Cargo.toml.template          ~3KB      ⭐⭐⭐

Total Package Size: ~34KB (compressed)
```

---

## 💡 Tips for Using This Setup

1. **Start with SETUP_GUIDE.md**: It explains how everything works together

2. **Reference context.md liberally**: It's Claude's knowledge base
   ```
   "As described in .claude/context.md section X..."
   ```

3. **Follow architecture.md patterns**: For consistent design
   ```
   "Following the architecture in .claude/architecture.md..."
   ```

4. **Apply conventions.md standards**: For quality code
   ```
   "Use conventions from .claude/conventions.md..."
   ```

5. **Use prompts.md templates**: Don't start from scratch
   ```
   Copy, customize, paste
   ```

6. **Track in memory.md**: Document decisions
   ```
   "Update .claude/memory.md with decision..."
   ```

7. **Check off checklist.md**: Stay organized
   ```
   Daily progress tracking
   ```

---

## 🔍 Finding Information Quickly

**Need to know about...?**

- **Bond pricing methodology** → `.claude/context.md` (section: Pricing Methodologies)
- **System architecture** → `.claude/architecture.md` (diagrams section)
- **Coding standards** → `.claude/conventions.md` (specific section)
- **How to ask Claude** → `.claude/prompts.md` (find similar task)
- **Past decisions** → `.claude/memory.md` (decisions log)
- **What's next** → `.claude/checklist.md` (current phase)
- **Setup process** → `.claude/quickstart.md` or `SETUP_GUIDE.md`
- **Big picture** → `OVERVIEW.md`

---

## 📞 Support

**Can't find something?**
1. Check this INDEX.md for overview
2. Read SETUP_GUIDE.md for detailed explanations
3. Use Ctrl+F / Cmd+F to search within files
4. All files are interconnected - follow references

**Files reference each other**:
- context.md ← referenced by prompts
- architecture.md ← referenced by conventions
- conventions.md ← referenced by prompts
- memory.md ← updated by everything
- checklist.md ← guides daily work

---

## ✅ Checklist for Using This Index

- [ ] Read OVERVIEW.md for big picture
- [ ] Read SETUP_GUIDE.md for detailed explanation
- [ ] Understand purpose of each file
- [ ] Know which files to read first
- [ ] Know which files to update regularly
- [ ] Know how to search for information
- [ ] Bookmark this INDEX.md for quick reference

---

**Ready to build Convex!** 🚀

Start with `.claude/SETUP_GUIDE.md` for complete instructions.
