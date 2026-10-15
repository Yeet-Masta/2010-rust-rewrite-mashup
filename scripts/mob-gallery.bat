@echo off
rem Every mob on a flat stage in the sky (--mob-gallery): builds the game if
rem needed and opens a fresh creative world at noon (not saved) with all of
rem them standing still in rows, the sea creatures in a water channel at the
rem front. Fly around to look at them; extra options pass through.
cd /d "%~dp0\.."
cargo build --release || exit /b 1
target\release\minecraft.exe --mob-gallery %*
