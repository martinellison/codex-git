# codex-git

`codex-git` is a wrapper for git2 to simplify access to a git repository from
Rust code. `codex-git` runs on Android as well as PCs. See the API doco for more
information.

## Building

To get this to build for Android, I needed a `.cargo/config.toml` file with
something like (replacing `NDK` with the location of the Android NDK and 31 by
the required version of the NDK):

```
[target.aarch64-linux-android]
linker="NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android31-clang"

[target.x86_64-linux-android]
linker="NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android31-clang"

[target.armv7-linux-android]
linker="NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/armv7a-linux-androideabi31-clang"
```

This may vary depending on the version of the NDK.
