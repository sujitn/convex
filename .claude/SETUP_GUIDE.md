# Convex Project Setup - Complete Guide

## Overview

This directory contains a complete Claude Code setup for building **Convex**, a production-grade fixed income analytics library in Rust. This guide explains what each file does and how to use them effectively with Claude Code.

## Files Created

### 1. Core Documentation Files (`.claude/` directory)

#### `.claude/context.md` (Most Important!)
**Purpose**: Provides comprehensive domain knowledge about fixed income analytics.

**Contains**:
- Fixed income fundamentals (bond types, pricing methodologies)
- Complete list of analytics requirements
- Technical architecture principles
- Project structure
- Performance targets
- Bloomberg YAS methodology details
- Academic and industry references

**When Claude Reads This**: It understands the domain deeply and can make informed decisions about implementation approaches.

#### `.claude/architecture.md`
**Purpose**: Visual system architecture and design patterns.

**Contains**:
- System architecture diagram (Mermaid)
- Component architecture with class diagrams
- Data flow diagrams
- Module dependency graphs
- Performance optimization strategies
- Testing strategies
- Deployment architecture

**When Claude Reads This**: It understands how components fit together and can design consistent APIs.

#### `.claude/memory.md`
**Purpose**: Track decisions, progress, and known issues.

**Contains**:
- Architecture decisions log (with rationale)
- Technical decisions (dependencies, frameworks)
- Domain decisions (which features first)
- Implementation progress tracking
- Known issues and challenges
- Performance metrics tracking
- Q&A section for open questions

**How to Use**: Update this as the project progresses. Claude can reference past decisions and maintain consistency.

#### `.claude/conventions.md`
**Purpose**: Rust coding standards and best practices.

**Contains**:
- Code formatting rules
- Naming conventions
- Type safety patterns
- Error handling standards
- Documentation requirements
- Performance best practices
- Testing conventions
- Concurrency patterns

**When Claude Reads This**: It writes idiomatic Rust code that follows project standards.

#### `.claude/prompts.md`
**Purpose**: Example prompts for common development tasks.

**Contains**:
- Getting started prompts
- Domain-specific prompts (curves, spreads, bonds)
- Testing and optimization prompts
- Debugging prompts
- Code review prompts
- Maintenance prompts

**How to Use**: Copy/paste and adapt these prompts for your specific needs.

#### `.claude/quickstart.md`
**Purpose**: Step-by-step guide to get started.

**Contains**:
- Installation prerequisites
- Project initialization steps
- Development workflow examples
- Common development tasks
- Troubleshooting tips
- Next steps

**How to Use**: Follow this guide when setting up the project for the first time.

#### `.claude/checklist.md`
**Purpose**: Detailed development checklist by phase.

**Contains**:
- Phase-by-phase task breakdown
- Acceptance criteria for each milestone
- Testing requirements
- Performance targets
- Documentation requirements

**How to Use**: Track progress, ensure nothing is missed, guide development priorities.

### 2. Template Files

#### `Cargo.toml.template`
**Purpose**: Workspace configuration template.

**Contains**:
- Workspace member definitions
- Shared dependencies
- Build profiles (dev, release, bench)
- Optimization settings
- Platform-specific configurations

**How to Use**: Copy to `Cargo.toml` and customize for your project.

#### `README.md.template`
**Purpose**: Project README template.

**Contains**:
- Project description and features
- Installation instructions
- Quick start examples
- Architecture overview
- Performance benchmarks
- Roadmap
- Contributing guidelines

**How to Use**: Copy to `README.md` and customize with your details.

## Using This Setup with Claude Code

### Initial Setup (First Time)

1. **Install Prerequisites**:
   ```bash
   # Install Rust
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Install Claude Code
   curl -fsSL https://claude.ai/download/code | sh
   ```

2. **Create Project Directory**:
   ```bash
   mkdir convex
   cd convex
   ```

3. **Copy Downloaded Files**:
   ```bash
   # Copy the entire .claude directory
   cp -r /path/to/downloaded/.claude .
   
   # Copy templates
   cp /path/to/downloaded/Cargo.toml.template Cargo.toml
   cp /path/to/downloaded/README.md.template README.md
   ```

4. **Initialize Git**:
   ```bash
   git init
   git add .claude/ Cargo.toml README.md
   git commit -m "Initial project setup with Claude Code configuration"
   ```

### Starting Development with Claude Code

1. **Launch Claude Code**:
   ```bash
   cd convex
   claude code
   ```

2. **First Prompt** (Copy exactly):
   ```
   I'm starting the Convex fixed income analytics library. Please read the following files to understand the project:
   - .claude/context.md (domain knowledge)
   - .claude/architecture.md (system design)
   - .claude/conventions.md (coding standards)
   
   Then, create the initial Cargo workspace structure with all crates:
   - convex-core (core types and traits)
   - convex-curves (yield curve construction)
   - convex-bonds (bond pricing)
   - convex-spreads (spread calculations)
   - convex-math (mathematical utilities)
   - convex-ffi (FFI layer)
   
   Set up each crate with:
   - Appropriate Cargo.toml
   - Basic module structure (lib.rs with module declarations)
   - Essential dependencies
   - Initial README.md
   
   Follow the architecture in .claude/architecture.md and conventions in .claude/conventions.md.
   ```

3. **Claude Will**:
   - Read all the context files
   - Understand the domain and requirements
   - Create the workspace structure
   - Set up initial crates
   - Follow Rust best practices
   - Include appropriate documentation

### Development Workflow

#### Pattern 1: Implementing a New Feature

```
Implement [FEATURE] in [CRATE]:

Requirements (reference .claude/context.md sections):
- [Specific requirement 1]
- [Specific requirement 2]

Follow:
- Architecture patterns in .claude/architecture.md
- Coding conventions in .claude/conventions.md
- Testing standards in .claude/conventions.md

Include:
- Comprehensive unit tests (>90% coverage)
- Documentation with examples
- Performance benchmarks if applicable
```

#### Pattern 2: Getting Unstuck

```
I'm working on [PROBLEM]. Please:
1. Review the relevant section in .claude/context.md
2. Check if there's guidance in .claude/architecture.md
3. Suggest an implementation approach
4. Provide example code following .claude/conventions.md
```

#### Pattern 3: Code Review

```
Review this implementation for:
1. Correctness (matches .claude/context.md requirements)
2. Architecture (follows .claude/architecture.md patterns)
3. Code quality (adheres to .claude/conventions.md)
4. Performance (meets targets in .claude/context.md)
5. Testing (sufficient coverage per .claude/conventions.md)
```

#### Pattern 4: Tracking Progress

```
Please update .claude/memory.md with:
- Decision: [What was decided]
- Rationale: [Why this approach]
- Alternatives: [What else was considered]
- Status: [Approved/Pending]
```

### Best Practices for Claude Code

1. **Always Reference Documentation**:
   - "As described in .claude/context.md..."
   - "Following the architecture in .claude/architecture.md..."
   - "Using conventions from .claude/conventions.md..."

2. **Be Specific About Requirements**:
   - Reference specific sections in context.md
   - Mention performance targets
   - Specify validation criteria

3. **Request Comprehensive Outputs**:
   - Don't just ask for code
   - Request tests, docs, examples
   - Ask for benchmarks when relevant

4. **Incremental Development**:
   - Start with basic implementation
   - Add features incrementally
   - Refine based on feedback

5. **Keep Memory Updated**:
   - Ask Claude to update memory.md with decisions
   - Document rationale
   - Track progress on checklist.md

### Example Development Sequence

#### Week 1: Core Types

```
Phase 1: Implement core types in convex-core

1. First, implement the Date type:
   - Date struct with year/month/day
   - Date arithmetic (add_days, add_months)
   - Serialization support
   - Comprehensive tests
   - Documentation with examples
   
   Follow the type safety patterns in .claude/conventions.md

2. Then implement Price, Yield, and Spread types as described in .claude/context.md

3. Update .claude/memory.md with any design decisions made
```

#### Week 2: Day Count Conventions

```
Implement all day count conventions in convex-core:

Reference .claude/context.md section "Day Count Conventions (Must be exact)"

Implement:
1. DayCounter trait
2. ACT/360 (money market)
3. ACT/365 (UK Gilts)
4. 30/360 US (corporate bonds)
5. ACT/ACT ICMA
6. ACT/ACT ISDA

For each:
- Exact implementation matching Bloomberg
- Edge case handling (leap years, month-end)
- Comprehensive tests with known values
- Documentation with formulas

Check off items in .claude/checklist.md as completed
```

#### Week 3: Yield Curves

```
Implement yield curve construction in convex-curves:

Follow the "Yield Curve Construction Flow" diagram in .claude/architecture.md

Requirements from .claude/context.md:
- Bootstrap from deposits and bonds
- Linear and cubic spline interpolation
- Cache interpolation coefficients
- Target: <100μs for 50-point curve

Include:
- Comprehensive tests with market data
- Benchmarks measuring performance
- Validation against known curves
```

## Troubleshooting

### Claude Isn't Following Conventions

**Solution**: Explicitly reference the conventions file:
```
The code doesn't follow the conventions. Please review .claude/conventions.md 
and ensure:
- Proper error handling with thiserror
- Type safety with newtypes
- Documentation with examples
- Unit tests with >90% coverage
```

### Claude Doesn't Understand Domain

**Solution**: Point to specific context sections:
```
I need help with Z-spread calculation. Please:
1. Read the "Spread Analytics" section in .claude/context.md
2. Review the "Spread Calculation Architecture" in .claude/architecture.md
3. Implement following .claude/conventions.md standards
```

### Performance Issues

**Solution**: Reference performance requirements:
```
The implementation is too slow. According to .claude/context.md, we need:
- Single bond price: <1 microsecond
- YTM calculation: <10 microseconds

Please:
1. Profile the code
2. Identify bottlenecks
3. Apply optimizations from .claude/conventions.md
4. Create benchmarks
```

### Missing Tests

**Solution**: Reference testing standards:
```
Add comprehensive tests following .claude/conventions.md:
- Unit tests for each function
- Property tests for invariants
- Integration tests with real bond data
- Validation tests vs Bloomberg
- Target: >90% coverage
```

## Maintenance

### Regular Updates

1. **Update Memory** (Weekly):
   ```
   Update .claude/memory.md with:
   - Decisions made this week
   - Progress on milestones
   - Known issues discovered
   - Performance metrics
   ```

2. **Review Checklist** (Daily):
   ```
   Update .claude/checklist.md by marking completed items
   ```

3. **Document Decisions** (As Made):
   ```
   Add new architectural decision to .claude/memory.md:
   - Decision: [What]
   - Rationale: [Why]
   - Alternatives: [Other options]
   - Impact: [Consequences]
   ```

### Version Control

Commit the .claude directory changes:
```bash
git add .claude/memory.md
git commit -m "Update memory with YTM solver decisions"
```

## Tips for Success

1. **Read Context First**: The more context Claude has, the better the output
2. **Be Specific**: Vague requests get vague results
3. **Iterate**: Start simple, refine incrementally
4. **Test Everything**: Request tests with every implementation
5. **Document Decisions**: Update memory.md regularly
6. **Track Progress**: Use checklist.md to stay organized
7. **Reference Standards**: Always point to Bloomberg/academic sources

## Getting Help

If you get stuck:

1. Check `.claude/quickstart.md` for common tasks
2. Look in `.claude/prompts.md` for prompt examples
3. Reference `.claude/context.md` for domain knowledge
4. Review `.claude/conventions.md` for code patterns
5. Consult `.claude/architecture.md` for design patterns

## Summary

This setup provides Claude Code with:
- **Complete domain knowledge** (context.md)
- **System architecture** (architecture.md)
- **Coding standards** (conventions.md)
- **Example prompts** (prompts.md)
- **Getting started guide** (quickstart.md)
- **Progress tracking** (memory.md, checklist.md)

With these files, Claude Code can:
- Make informed technical decisions
- Write production-quality code
- Follow best practices
- Maintain consistency
- Meet performance requirements
- Match industry standards

**Ready to build Convex!** 🚀

Start with: `claude code` and use the first prompt from the "Starting Development" section above.
