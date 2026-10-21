# Archive fixtures (used by `src/archive.rs` tests)

| File | Origin |
| :--- | :--- |
| `rar5_normal.rar` | libarchive test vector `test_read_format_rar5_stored.rar` |
| `rar4_symlink.rar` | libarchive `test_read_format_rar.rar` (RAR4, has a symlink entry) |
| `rar5_symlink.rar` | libarchive `test_read_format_rar5_symlink.rar` |
| `rar5_hardlink.rar` | libarchive `test_read_format_rar5_hardlink.rar` |
| `rar5_traversal.rar` | `rar5_normal.rar` with the entry renamed to `../loworld.txt` (header CRC32 recomputed) |
| `rar5_absolute.rar` | `rar5_normal.rar` with the entry renamed to `/tmp/world.txt` (header CRC32 recomputed) |
| `normal.7z`, `symlink.7z`, `hardlink.7z` | p7zip `7z a`, `7z a -snl`, `7z a -snh` |
| `tar_normal.tar.gz`, `tar_hardlink.tar.gz` | GNU tar `-czf` (the second stores a hard link) |

libarchive test vectors are from libarchive v3.8.1 (`libarchive/test/*.rar.uu`, BSD-2-Clause), uudecoded.
Adversarial zip and 7z archives are generated inside the tests.
