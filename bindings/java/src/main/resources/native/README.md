# Native libraries

This directory is populated at build time with the `convex-ffi` cdylib for each
supported platform, one subdirectory per classifier:

```
native/
  linux-x86_64/libconvex_ffi.so
  linux-aarch64/libconvex_ffi.so
  darwin-x86_64/libconvex_ffi.dylib
  darwin-aarch64/libconvex_ffi.dylib
  windows-x86_64/convex_ffi.dll
```

Build one with, e.g.:

```
cargo build -p convex-ffi --release
cp target/release/libconvex_ffi.so \
   bindings/java/src/main/resources/native/linux-x86_64/
```

CI builds all targets in a matrix and stages them here before `mvn package`.
`NativeLoader` extracts the matching file at runtime.
