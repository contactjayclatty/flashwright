# PixelFlasher reuse

Flashwright is AGPL-3.0-or-later. PixelFlasher is AGPL-3.0-or-later. Copyright for the upstream files stays with badabing2005.

Python files from PixelFlasher may be copied when a behaviour has to stay line-for-line. Each copied file keeps its AGPL header. The copy is listed in this file and in `docs/disclaimer.md`, with the upstream path, the commit, and the licence.

The `bin/` directory of PixelFlasher is not copied. It holds bundled programs and is not source we relicense or vendor.

This change does not copy any new `.py` file. The safety tables already ported from commit `081286d` are behaviour ports, recorded in `docs/disclaimer.md`. Gate G26 refuses a vbmeta flash. That is Flashwright policy and is the opposite of PixelFlasher's `flash_vbmeta_if_needed`.
