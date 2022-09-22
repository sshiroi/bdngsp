# vapoursynth BD source thats Not Good but should Suffice for Preview
Vapoursynth Source filter I wrote that uses the CLPI files for keyframe position and estimates framenumbers from there.
Will probably break if you throw anything other than single video h264 yuv420p8 progressive stuff at it.
Returns the same frame as lsmas only most of the time.
Also I don't know what im doing so beware.
Please don't acually use this for anything other than preview.
Or don't use this at all only I only realized this is a bad idea when I was already halfway done.

## Usage
```
video = core.bdngsp.Source("<...>/BDMV/STREAM/00000.m2ts")
```