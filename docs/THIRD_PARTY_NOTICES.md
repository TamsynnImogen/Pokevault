# Third-party interface notices

The Rust Libretro ABI adapter follows the public v1 interface described in
[libretro.h](https://github.com/libretro/libretro-common/blob/master/include/libretro.h).
Its interface notice is retained below. This does not relicense mGBA or claim
that a core binary is bundled or distribution-cleared.

## Libretro API header

Copyright (C) 2010-2024 The RetroArch team

Permission is hereby granted, free of charge,
to any person obtaining a copy of this software and associated documentation files (the "Software"),
to deal in the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software,
and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## mGBA runtime

The application loads a user-installed mGBA Libretro core; it does not distribute
the core, its frontend, BIOS or ROM assets. See [PLAYABLE_EMULATOR.md](PLAYABLE_EMULATOR.md)
and [ARCHITECTURE.md](ARCHITECTURE.md) for the current API and distribution strategy.
The upstream core license is [MPL-2.0](https://github.com/mgba-emu/mgba/blob/0.10.5/LICENSE).
