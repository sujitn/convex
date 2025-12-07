# convex-ffi

C FFI bindings for the Convex fixed income analytics library.

## Overview

`convex-ffi` provides C-compatible foreign function interface bindings for the Convex library, enabling integration with:

- Python (via ctypes or CFFI)
- Java (via JNI)
- C# (via P/Invoke)
- Excel (via XLL)
- Any language with C FFI support

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
convex-ffi = "0.1"
```

## Building

### Shared Library

```bash
cargo build --release -p convex-ffi
```

This produces:
- Linux: `libconvex_ffi.so`
- macOS: `libconvex_ffi.dylib`
- Windows: `convex_ffi.dll`

### Static Library

```bash
cargo build --release -p convex-ffi
```

This also produces:
- Linux/macOS: `libconvex_ffi.a`
- Windows: `convex_ffi.lib`

### C Header Generation

The build automatically generates `convex.h` using cbindgen.

## C API

### Bond Pricing

```c
#include "convex.h"

// Create a fixed rate bond
ConvexBond* bond = convex_bond_create(
    "097023AH7",        // CUSIP
    0.075,              // Coupon rate
    20250615,           // Maturity (YYYYMMDD)
    2,                  // Frequency (2 = semi-annual)
    DAYCOUNT_30_360_US  // Day count convention
);

// Calculate yield to maturity
double ytm;
ConvexError err = convex_bond_ytm(bond, 110.503, 20240429, &ytm);
if (err != CONVEX_OK) {
    const char* msg = convex_error_message(err);
    fprintf(stderr, "Error: %s\n", msg);
}

// Calculate accrued interest
double accrued;
convex_bond_accrued(bond, 20240429, &accrued);

// Clean up
convex_bond_free(bond);
```

### Curve Operations

```c
// Create a discount curve
ConvexCurve* curve = convex_curve_create(20240115);

// Add instruments
convex_curve_add_deposit(curve, "1M", 0.0525);
convex_curve_add_deposit(curve, "3M", 0.0535);
convex_curve_add_swap(curve, "2Y", 0.0545);
convex_curve_add_swap(curve, "5Y", 0.0560);

// Build the curve
convex_curve_build(curve);

// Get discount factor
double df;
convex_curve_discount_factor(curve, 3.0, &df);

// Clean up
convex_curve_free(curve);
```

### Error Handling

```c
ConvexError err = convex_some_operation(...);

switch (err) {
    case CONVEX_OK:
        // Success
        break;
    case CONVEX_ERROR_INVALID_INPUT:
        // Handle invalid input
        break;
    case CONVEX_ERROR_SOLVER_FAILED:
        // Handle solver failure
        break;
    default:
        // Handle other errors
        break;
}

// Get error message
const char* msg = convex_error_message(err);
```

## Python Integration

```python
import ctypes

# Load the library
lib = ctypes.CDLL("./libconvex_ffi.so")

# Define function signatures
lib.convex_bond_create.argtypes = [
    ctypes.c_char_p, ctypes.c_double, ctypes.c_int,
    ctypes.c_int, ctypes.c_int
]
lib.convex_bond_create.restype = ctypes.c_void_p

lib.convex_bond_ytm.argtypes = [
    ctypes.c_void_p, ctypes.c_double, ctypes.c_int,
    ctypes.POINTER(ctypes.c_double)
]
lib.convex_bond_ytm.restype = ctypes.c_int

# Create a bond
bond = lib.convex_bond_create(
    b"097023AH7", 0.075, 20250615, 2, 0
)

# Calculate YTM
ytm = ctypes.c_double()
err = lib.convex_bond_ytm(bond, 110.503, 20240429, ctypes.byref(ytm))
print(f"YTM: {ytm.value:.6%}")

# Clean up
lib.convex_bond_free(bond)
```

## Thread Safety

All FFI functions are thread-safe. The underlying Rust implementation uses appropriate synchronization primitives.

## Memory Management

- Functions returning opaque pointers (e.g., `ConvexBond*`, `ConvexCurve*`) allocate memory that must be freed with the corresponding `_free` function.
- String outputs use caller-provided buffers or return static strings.
- Error messages from `convex_error_message` are static and should not be freed.

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
