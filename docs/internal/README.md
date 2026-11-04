# Project tracking notes

These files track the project's own engineering work: the CI assurance rules,
the inventory of library operations still to be covered, APIs that were removed
and what replaced them, and how the Java host checks run. They are kept for
contributors and for the tools under `tools/library/`, which check that they
exist.

If you are using Convex rather than working on it, start with the
[main README](../../README.md) and the [documentation index](../README.md).

- [ASSURANCE.md](ASSURANCE.md): the coding rules CI enforces and how they are checked.
- [LIBRARY-COVERAGE.md](LIBRARY-COVERAGE.md): what the library covers so far, area by area.
- [CVX-063-REMAINING.md](CVX-063-REMAINING.md): the work still open on the library API.
- [ROUTE-MIGRATION.md](ROUTE-MIGRATION.md): removed convenience functions and their replacements.
- [JAVA-QUALIFICATION.md](JAVA-QUALIFICATION.md): running the Java binding checks.
