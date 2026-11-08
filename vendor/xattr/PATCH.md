This is xattr 0.2.3 from crates.io, under its original MIT/Apache-2.0 licenses.
It is required transitively by GPUI's async-tar dependency.

One compatibility change in `src/sys/mod.rs` maps ENOATTR to libc::ENODATA on
Linux/Android, where ENOATTR was a deprecated alias removed in recent libc releases.
Other targets retain their original constant. No xattr behavior changes.

Remove this patch when the GPUI dependency chain upgrades async-tar/xattr.
