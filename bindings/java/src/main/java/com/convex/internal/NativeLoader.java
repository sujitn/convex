package com.convex.internal;

import com.convex.ConvexException;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Locale;

/**
 * Locates and loads the bundled {@code convex-ffi} cdylib for the running
 * platform.
 *
 * <p>The native libraries ship inside the jar under
 * {@code /native/<classifier>/<libname>}; this extracts the matching one to a
 * temp file (libraries can only be loaded from the real filesystem) and calls
 * {@link System#load(String)}. Loading once per JVM is enough — the
 * {@link ConvexFfi} static initializer drives it.
 */
final class NativeLoader {

    private NativeLoader() {}

    private static volatile boolean loaded = false;

    static synchronized void ensureLoaded() {
        if (loaded) {
            return;
        }
        String classifier = classifier();
        String libName = libName(classifier);
        String resource = "/native/" + classifier + "/" + libName;

        try (InputStream in = NativeLoader.class.getResourceAsStream(resource)) {
            if (in == null) {
                throw new ConvexException(
                        "no bundled native library for this platform: " + resource
                                + " (build crates/convex-ffi for this target and stage it under"
                                + " src/main/resources/native/<classifier>/)");
            }
            Path tmpDir = Files.createTempDirectory("convex-native");
            Path target = tmpDir.resolve(libName);
            Files.copy(in, target, StandardCopyOption.REPLACE_EXISTING);
            target.toFile().deleteOnExit();
            tmpDir.toFile().deleteOnExit();
            System.load(target.toAbsolutePath().toString());
            loaded = true;
        } catch (IOException e) {
            throw new ConvexException("error", "failed to extract native library: " + e.getMessage(), null);
        } catch (UnsatisfiedLinkError | SecurityException e) {
            throw new ConvexException(
                    "error", "failed to load native library " + resource + ": " + e.getMessage(), null);
        }
    }

    /** {@code <os>-<arch>} matching the resource directory layout. */
    private static String classifier() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);

        String osTag;
        if (os.contains("win")) {
            osTag = "windows";
        } else if (os.contains("mac") || os.contains("darwin")) {
            osTag = "darwin";
        } else if (os.contains("nux") || os.contains("nix")) {
            osTag = "linux";
        } else {
            throw new ConvexException("unsupported OS: " + os);
        }

        String archTag;
        if (arch.equals("amd64") || arch.equals("x86_64")) {
            archTag = "x86_64";
        } else if (arch.equals("aarch64") || arch.equals("arm64")) {
            archTag = "aarch64";
        } else {
            throw new ConvexException("unsupported architecture: " + arch);
        }

        return osTag + "-" + archTag;
    }

    private static String libName(String classifier) {
        if (classifier.startsWith("windows")) {
            return "convex_ffi.dll";
        } else if (classifier.startsWith("darwin")) {
            return "libconvex_ffi.dylib";
        } else {
            return "libconvex_ffi.so";
        }
    }
}
