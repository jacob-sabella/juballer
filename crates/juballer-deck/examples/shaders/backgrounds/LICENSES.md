# Background shader attributions

These shaders are ports of Shadertoy entries. Per the
[Shadertoy FAQ](https://www.shadertoy.com/terms), shaders without an
explicit license are licensed **CC BY-NC-SA 3.0** — attribution required,
non-commercial use only, derivative works must share alike.

juballer is a personal / non-commercial project. If you ship juballer
commercially you must either remove these backgrounds, replace them
with original work, or obtain separate licenses from the authors.

Each port adapts Shadertoy's `iChannel0` audio-texture reads to a
synthesized `game_audio(x)` helper driven by juballer's own gameplay
channels (beat phase, combo, life, last-hit flash).

| file                  | source                                       | author       | title                              |
|-----------------------|----------------------------------------------|--------------|------------------------------------|
| `bg_oscilloscope.wgsl`| https://www.shadertoy.com/view/slc3DX        | incription   | Oscilloscope music (2021)          |
| `bg_fractal.wgsl`     | https://www.shadertoy.com/view/llB3W1        | shadertoy user | Fractal Audio 01                 |
| `bg_cyber_fuji.wgsl`  | https://www.shadertoy.com/view/fd2GRw        | shadertoy user | Cyber Fuji 2020 audio reactive   |
| `bg_dancing_cubes.wgsl`| https://www.shadertoy.com/view/MsdBR8       | shadertoy user (based on Shane's Raymarched Reflections, https://www.shadertoy.com/view/4dt3zn) | dancing cubes |
| `bg_galaxy.wgsl`      | https://www.shadertoy.com/view/NdG3zw        | "CBS" (inspired by JoshP's Simplicity, https://www.shadertoy.com/view/lslGWr) | Audio Reactive Galaxy |
| `bg_inversion.wgsl`   | https://www.shadertoy.com/view/4dsGD7        | Kali         | The Inversion Machine              |

All ports © original authors under CC BY-NC-SA 3.0. Port adaptations © juballer project under the same terms.
