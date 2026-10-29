// Measured on the spare by psi-probe (2026-10-26, 1.0.0-beta.10-anylook-a): VM p50 µs a frame.
pub const STEPS: &[(&str, &str, u32)] = &[
    ("base gray(x)", "gray(x)", 678),
    ("add", "gray(x + t)", 897),
    ("mul", "gray(x * t)", 1041),
    ("div (+add)", "gray(x / (x + t))", 2072),
    // fast-div (1.0.0-beta.11-fastdiv-b, 2026-10-29): these four rows again,
    // and a division taking the Knuth digit.
    ("div 128/v (+add)", "gray(128 / (px + t))", 2644),
    ("mod 0.3", "gray(x % 0.3)", 1152),
    ("sin (+add)", "gray(sin(x + t))", 1346),
    ("sqrt (+add)", "gray(sqrt(x + t))", 1789),
    ("len (+add)", "gray(len(x, x + t))", 2799),
    ("atan2 (+sub)", "gray(atan2(y - 0.5, x - t))", 2452),
    ("smoothstep (+add)", "gray(smoothstep(0.2, 0.8, x + t))", 1782),
    ("noise (+mul)", "gray(noise(x * 8, y, t))", 4540),
    ("select (+lt)", "gray(x < t ? x : 0.5)", 1064),
    ("digit (+2 mask)", "gray(digit(px % 8, px % 4, y * 6))", 2066),
    ("bit", "gray(bit(px, t))", 915),
    ("pick (+mul)", "gray(pick(x * 4, t, 0.5, 0.75, 1))", 1452),
    ("pick 8 (+mul)", "gray(pick(x * 8, t, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7))", 1452),
    ("chain 4 (+mul)", "k = x * 4\ngray(k < 0 ? 0 : k < 1 ? t : k < 2 ? 0.5 : k < 3 ? 0.75 : k < 4 ? 1 : 0)", 3015),
    ("big clock", "loop 60\n-- Big clock: the time in four big digits, and a seconds bar along the bottom\nframe {\n  h1 = floor(hour/10); h2 = hour % 10       -- the hours' two digits\n  m1 = floor(minute/10); m2 = minute % 10   -- the minutes' two digits\n  end = (second + 1)*128/60                 -- where the seconds bar ends\n}\n-- a digit: the bold font's 3 x 5 pixels, each 8 px wide, 6 tall (24 x 30)\nq = px >= 64 ? px - 8 : px     -- the minutes sit 8 px further right\nk = floor(q/32)                -- which digit: one every 32 px (0 1, then 2 3)\nn = pick(k, h1, h2, m1, m2)    -- and its value\ncol = floor(q % 32 / 8)        -- the font's column, 0 to 2 (then the gap)\nrow = floor(ty/6)              -- its row from the top, 0 to 4 (5: below)\nbar = py < 1                   -- the bottom row is the seconds bar\nlit = bar ? px < end : digit(n, col, row, bold)\n-- a rainbow drifting through the digits, tilted by row; the bar is white\nhue = py/64 + hour/24 + t/10\nhsv(x + hue, bar ? 0 : 1, lit)", 6121),
    ("big clock (beta.10)", "loop 60\n-- Big clock: the time in four big digits, and a seconds bar along the bottom\nframe {\n  h1 = floor(hour/10); h2 = hour % 10       -- the hours' two digits\n  m1 = floor(minute/10); m2 = minute % 10   -- the minutes' two digits\n  end = (second + 1)*128/60                 -- where the seconds bar ends\n}\n-- a digit: the bold font's 3 x 5 pixels, each 8 px wide, 6 tall (24 x 30)\nq = px >= 64 ? px - 8 : px     -- the minutes sit 8 px further right\nk = floor(q/32)                -- which digit: one every 32 px (0 1, then 2 3)\nn = k < 1 ? h1 : k < 2 ? h2 : k < 3 ? m1 : m2   -- and its value\ncol = floor(q % 32 / 8)        -- the font's column, 0 to 2 (then the gap)\nrow = floor((31 - py)/6)       -- its row from the top, 0 to 4 (5: below)\nbar = py < 1                   -- the bottom row is the seconds bar\nlit = bar ? px < end : digit(n, col, row, bold)\n-- a rainbow drifting through the digits, tilted by row; the bar is white\nhue = py/64 + hour/24 + t/10\nhsv(x + hue, bar ? 0 : 1, lit)", 7275),
    ("out hsv", "hsv(x, 1, 1)", 1511),
    ("out pal", "pal(fire, x)", 610),
    ("ur", "gray(ur)", 962),
    ("ul", "gray(ul)", 1256),
    ("shows through", "gray(x, 0.5)", 2135),
    ("+layer over 0.5", "layer { gray(x) }\nlayer { gray(x + t, 0.5) }", 2949),
    ("+layer add", "layer { gray(x) }\nlayer add { gray(x + t) }", 2128),
    ("+layer multiply", "layer { gray(x) }\nlayer multiply { gray(x + t) }", 2349),
    ("+layer screen", "layer { gray(x) }\nlayer screen { gray(x + t) }", 2575),
    ("+layer max", "layer { gray(x) }\nlayer max { gray(x + t) }", 2077),
    ("look scanlines", "layer multiply { gray(py % 2 ? 1 : 0.55) }", 1098),
    ("look 4 layers", "layer multiply { gray(y) }\nlayer add { gray(x) }\nlayer screen { gray(x + t) }\nlayer max { gray(py / 32) }", 4957),
    ("+layer add 0.4", "layer { gray(x) }\nlayer add { gray(x + t, 0.4) }", 3707),
    ("+layer multiply 0.4", "layer { gray(x) }\nlayer multiply { gray(x + t, 0.4) }", 3929),
    ("+layer screen 0.4", "layer { gray(x) }\nlayer screen { gray(x + t, 0.4) }", 3789),
    ("+layer max 0.4", "layer { gray(x) }\nlayer max { gray(x + t, 0.4) }", 3174),
    ("+layer min 0.4", "layer { gray(x) }\nlayer min { gray(x + t, 0.4) }", 3191),
    ("+layer screen row", "layer { gray(x) }\nlayer screen { gray(y + t) }", 1918),
    ("+layer screen hsv", "layer { gray(x) }\nlayer screen { hsv(x + t, 1, 1) }", 3407),
    ("screen 0.4 on picture", "layer screen { gray(x + t, 0.4) }", 3193),
    ("hsv 0.3 on picture", "hsv(x + t, 1, 1, 0.3)", 3186),
    ("screen hsv 0.4 on picture", "layer screen { hsv(x + t, 1, 1, 0.4) }", 4405),
];
pub const LOOKS: &[(&str, &str, &str, u8, u32)] = &[
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", "screen", 40, 7474),
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", "over", 30, 6257),
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", "add", 40, 7015),
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", "multiply", 100, 5653),
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", "max", 50, 6858),
    ("Fire", "loop 4\n-- curl bends the flames, more and more as they rise\ncurl = sin(y*1.5 - t)*0.25 + sin(y*3 - t*2 + x*4)*0.15\nflames = sin(x*6 + curl)*0.5 + sin(x*11 - curl)*0.5\nheat = (1 - y)*(1.6 + flames*0.7) - 0.5\npal(fire, clamp(heat, 0, 0.95))", "screen", 40, 8889),
    ("Fire", "loop 4\n-- curl bends the flames, more and more as they rise\ncurl = sin(y*1.5 - t)*0.25 + sin(y*3 - t*2 + x*4)*0.15\nflames = sin(x*6 + curl)*0.5 + sin(x*11 - curl)*0.5\nheat = (1 - y)*(1.6 + flames*0.7) - 0.5\npal(fire, clamp(heat, 0, 0.95))", "over", 30, 8054),
    ("Rings", "loop 4\n-- r: the distance from the middle\npal(rainbow, frac(r*1.5 - t/2))", "screen", 40, 6614),
    ("Stripes", "loop 4\n-- a: stripe width\nsize = 8 + a*24\nband = sin((px + py)/size - t/2)\ns = clamp(band*1.5 + 0.5, 0, 1)\nrgb(1, s, s)", "screen", 40, 5525),
    ("Copper bars", "loop 4\n-- each bar is brightest in its middle, in steps like the old chips\nred = floor(max(0, 1 - abs(cy - 0.35*sin(t/4))*5)*6)/6\ngold = floor(max(0, 1 - abs(cy - 0.35*sin(t/4 - 0.12))*5)*6)/6\nblue = floor(max(0, 1 - abs(cy - 0.35*sin(t/4 - 0.24))*5)*6)/6\nrgb(max(red, gold), max(gold*0.8, blue*0.6), blue)", "screen", 40, 2854),
    ("Old photo", "-- a: how much (0: the GIF as it is)\nmix(rgb(ur, ug, ub), rgb(ul*1.1 + 0.08, ul*0.92 + 0.04, ul*0.65), a)", "screen", 50, 9122),
    ("Old TV", "loop 4\ndx = (x - 0.5)*1.6; dy = (y - 0.5)*1.2\nlines = py % 2 ? 1 : 0.6\nroll = 0.85 + 0.15*max(0, tri(y/2 + t/4))\ncorners = clamp(1.5 - (dx*dx + dy*dy)*1.6, 0, 1)\nlayer multiply { gray(lines * roll * corners) }", "over", 100, 3958),
];
// The draw stage (psi-probe 2026-10-27, 1.0.0-beta.11-drawprobe-a): VM p50
// µs a frame, as STEPS.
pub const DRAW: &[(&str, &str, u32)] = &[
    (
        "draw base (1 dot off)",
        "draw { dot(-1, -1, t) }\ngray(ink)",
        931,
    ),
    (
        "draw loop step: 1024 dots off",
        "draw { for k < 1024 { dot(-1, -1, t) } }\ngray(ink)",
        1694,
    ),
    (
        "draw loop step: +add",
        "draw { for k < 1024 { dot(-1, -1, k + t) } }\ngray(ink)",
        2124,
    ),
    (
        "draw loop step: +hash",
        "draw { for k < 1024 { dot(-1, -1, hash(k, t)) } }\ngray(ink)",
        2235,
    ),
    (
        "draw dot on panel x1024",
        "draw { for k < 1024 { dot(k % 128, 5 + t) } }\ngray(ink)",
        2295,
    ),
    (
        "draw line 12 x256",
        "draw { for k < 256 { line(10, 5, 21, 5 + t) } }\ngray(ink)",
        2257,
    ),
    (
        "draw line 100 x64",
        "draw { for k < 64 { line(10, 5, 109, 5 + t) } }\ngray(ink)",
        3041,
    ),
];
// Formulas that draw played as effects (the real path, `Fx::frame`, slider
// a = 0.5): p50 µs a frame, compared with the estimate as it is (it includes
// the real paths' ×1.15). The sources are the examples as measured
// (1.0.0-beta.11-drawprobe-a), before the eye test's changes of 2026-10-27
// (opendmd-web/formulas/starfield.psi: longer streaks, heads off the panel skipped, squared
// brightness, `fps 100`; a bigger cube): kept, because a measurement is of
// its source. The new examples estimate 6.0 ms (Starfield) and 2.9 ms
// (cube); psi-probe times them at its next run.
pub const EFFECTS: &[(&str, &str, u32)] = &[
    ("Starfield", STARFIELD, 4410),
    ("Wireframe cube", CUBE, 2678),
];
pub const ROAD_SPRITE: &str = "-- author: OpenDMD\n-- licence: CC0-1.0\n-- bundle\nname \"Endless road\"\nabout \"OutRun into a synthwave sunset: bends and hills without end\"\nloop forever\nfps 100\n-- Endless road: an OutRun drive into a synthwave sunset. Bends and hills\n-- come from noise along the distance driven, which grows without end, so\n-- the track doesn't repeat; each time it plays it starts somewhere else.\n-- The colours, 0 to 18: the mountains and the car's glass and tyres\n-- (0), the road's stripes (1, 2), the kerbs' (3, 4), the verge's (5, 6),\n-- the sun from the top down (7 to 9), the car's body and tail lights\n-- (10, 11), then the sky from the top down to the horizon (12 to 18)\npalette road = #000, #400032, #800064, #e4cf00, #f00, #3f0000, #7f0000,\n  #e4cf00, #ff7800, #ff2800, #bf0000, #ff5000,\n  #1478ff, #2850ff, #3c28ff, #5000ff, #7b00f2, #a700e4, #d300d6\n-- The car, from behind: body (4), glass and tyres (5), tail lights (6)\nsprite car {\n  key . 0, R 4, D 5, L 6\n  ........RRRRRRRRRR........\n  .......RDDDDDDDDDDR.......\n  ......RDDDDDDDDDDDDR......\n  ...RRRRRRRRRRRRRRRRRRRR...\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  DDDDDDDDDDDDDDDDDDDDDDDDDD\n}\n-- The mountains: 19 spans (row, from, to)\ntable MY = 19,19,19,20,20,20,21,21,21,22,22,22,23,23,23,24,24,25,26\ntable MA = -1,57,118,0,59,120,2,61,122,4,62,123,5,64,125,7,66,67,69\ntable MB = 17,83,134,16,81,132,14,79,130,12,78,129,11,76,127,9,74,73,71\n-- Road rows py 0 to 18: depth/2 in bends (ZH), depth in stripes (ZS),\n-- half width in pixels (HW); the sun's rows and half widths\ntable ZH = .0058,.0061,.0064,.0068,.0072,.0077,.0083,.009,.0098,.011,.012,.013,.015,.017,.02,.025,.032,.045,.075\ntable ZS = .77,.81,.86,.91,.97,1.03,1.11,1.2,1.3,1.43,1.58,1.76,2,2.31,2.73,3.33,4.29,6,10\ntable HW = 50.7,48.1,45.5,42.9,40.3,37.7,35.1,32.5,29.9,27.3,24.7,22.1,19.5,16.9,14.3,11.7,9.1,6.5,3.9\ntable UY = 20,21,23,24,25,26,27,28,29,30,31\ntable UC = 7.2,7.6,8,8,7.9,7.6,7.2,6.7,6,5,3.5\nframe {\n  u = t*0.15 + seed*1000                    -- distance driven, in bends\n  h0 = noise(u, 0, 0); h1 = noise(u + 0.03, 0, 0)    -- heading here, just ahead\n  v0 = noise(u, 5, 0)                       -- height here, and its slope\n  dv = (noise(u + 0.01, 5, 0) - v0)*100\n  cx = 50.5 + (h1 - h0)*80                  -- the car's left edge: it slides out in bends\n  mx = (0.5 - h0)*60; sunx = 64 + mx        -- the sky turns with the road\n}\ndraw {\n  for k < 19 {\n    my = MY[k]\n    line(MA[k] + mx, my, MB[k] + mx, my, .35)\n  }\n  for k < 11 {\n    uy = UY[k]; uc = UC[k]\n    line(sunx - uc, uy, sunx + uc, uy, .2)\n  }\n}\n-- each row looks ahead: how far the road bends there, and how far a hill\n-- lifts or hides it (a crest moves rows to where the road is out of sight)\nzp = ZH[py]\nun = u + zp\nq = py - clamp((noise(un, 5, 0) - v0 - zp*dv)*ZS[py]*200, -6, 2)\nhw = HW[q]\nalt = frac(t*10 + ZS[q]) < 0.5             -- light or dark stripe: 10 a second\nadx = abs(px - (64 + (noise(un, 0, 0) - h0)*2000))\n-- 0 road, 1 kerb, 2 verge or sky; drawn: 3 the sun, 5 the mountains,\n-- 4 to 6 the car\nm = max(max((adx >= hw) + (adx >= hw*1.2), ink*16), car[px - cx, ty - 23])\n-- its colour, i/18 for colour i: the stripes', the sky's and the sun's by row\npal(road, pick(m, (1 + alt)/18, (3 + alt)/18,\n                  (py < 19 ? 5 + alt : floor(28.44 - py*.515))/18,\n                  (py < 23 ? 9 : py < 27 ? 8 : 7)/18, 10/18, 0, 11/18))\n";
pub const ROAD_DRAWN: &str = "-- author: OpenDMD\n-- licence: CC0-1.0\n-- bundle\nname \"Endless road\"\nabout \"OutRun into a synthwave sunset: bends and hills without end\"\nloop forever\nfps 100\n-- Endless road: an OutRun drive into a synthwave sunset. Bends and hills\n-- come from noise along the distance driven, which grows without end, so\n-- the track doesn't repeat; each time it plays it starts somewhere else.\n-- The colours, 0 to 18: the mountains and the car's glass and tyres\n-- (0), the road's stripes (1, 2), the kerbs' (3, 4), the verge's (5, 6),\n-- the sun from the top down (7 to 9), the car's body and tail lights\n-- (10, 11), then the sky from the top down to the horizon (12 to 18)\npalette road = #000, #400032, #800064, #e4cf00, #f00, #3f0000, #7f0000,\n  #e4cf00, #ff7800, #ff2800, #bf0000, #ff5000,\n  #1478ff, #2850ff, #3c28ff, #5000ff, #7b00f2, #a700e4, #d300d6\n-- The car, from behind: 17 spans (row, from, to, colour): body (4),\n-- glass and tyres (5), tail lights (6), drawn as ink 4/16 to 6/16\ntable CY = 8,7,7,6,6,5,4,4,4,4,3,3,3,3,2,1,0\ntable CA = 8,7,8,6,7,3,1,2,7,20,1,2,7,20,1,1,0\ntable CB = 17,18,17,19,18,22,24,5,18,23,24,5,18,23,24,24,25\ntable CI = 4,4,5,4,5,4,4,6,5,6,4,6,5,6,4,4,5\n-- The mountains: 19 spans (row, from, to)\ntable MY = 19,19,19,20,20,20,21,21,21,22,22,22,23,23,23,24,24,25,26\ntable MA = -1,57,118,0,59,120,2,61,122,4,62,123,5,64,125,7,66,67,69\ntable MB = 17,83,134,16,81,132,14,79,130,12,78,129,11,76,127,9,74,73,71\n-- Road rows py 0 to 18: depth/2 in bends (ZH), depth in stripes (ZS),\n-- half width in pixels (HW); the sun's rows and half widths\ntable ZH = .0058,.0061,.0064,.0068,.0072,.0077,.0083,.009,.0098,.011,.012,.013,.015,.017,.02,.025,.032,.045,.075\ntable ZS = .77,.81,.86,.91,.97,1.03,1.11,1.2,1.3,1.43,1.58,1.76,2,2.31,2.73,3.33,4.29,6,10\ntable HW = 50.7,48.1,45.5,42.9,40.3,37.7,35.1,32.5,29.9,27.3,24.7,22.1,19.5,16.9,14.3,11.7,9.1,6.5,3.9\ntable UY = 20,21,23,24,25,26,27,28,29,30,31\ntable UC = 7.2,7.6,8,8,7.9,7.6,7.2,6.7,6,5,3.5\nframe {\n  u = t*0.15 + seed*1000                    -- distance driven, in bends\n  h0 = noise(u, 0, 0); h1 = noise(u + 0.03, 0, 0)    -- heading here, just ahead\n  v0 = noise(u, 5, 0)                       -- height here, and its slope\n  dv = (noise(u + 0.01, 5, 0) - v0)*100\n  cx = 50.5 + (h1 - h0)*80                  -- the car's left edge: it slides out in bends\n  mx = (0.5 - h0)*60; sunx = 64 + mx        -- the sky turns with the road\n}\ndraw {\n  for k < 19 {\n    my = MY[k]\n    line(MA[k] + mx, my, MB[k] + mx, my, .35)\n  }\n  for k < 17 {\n    cy = CY[k]\n    line(cx + CA[k] + .49998, cy, cx + CB[k] + .49998, cy, CI[k]/16)\n  }\n  for k < 11 {\n    uy = UY[k]; uc = UC[k]\n    line(sunx - uc, uy, sunx + uc, uy, .2)\n  }\n}\n-- each row looks ahead: how far the road bends there, and how far a hill\n-- lifts or hides it (a crest moves rows to where the road is out of sight)\nzp = ZH[py]\nun = u + zp\nq = py - clamp((noise(un, 5, 0) - v0 - zp*dv)*ZS[py]*200, -6, 2)\nhw = HW[q]\nalt = frac(t*10 + ZS[q]) < 0.5             -- light or dark stripe: 10 a second\nadx = abs(px - (64 + (noise(un, 0, 0) - h0)*2000))\n-- 0 road, 1 kerb, 2 verge or sky; drawn: 3 the sun, 5 the mountains,\n-- 4 to 6 the car\nm = max((adx >= hw) + (adx >= hw*1.2), ink*16)\n-- its colour, i/18 for colour i: the stripes', the sky's and the sun's by row\npal(road, pick(m, (1 + alt)/18, (3 + alt)/18,\n                  (py < 19 ? 5 + alt : floor(28.44 - py*.515))/18,\n                  (py < 23 ? 9 : py < 27 ? 8 : 7)/18, 10/18, 0, 11/18))\n";
pub const STARFIELD: &str = "loop 2.4\n-- Starfield: 192 stars flying at you, a pass through the field a loop,\n-- each a streak from where it was a moment ago. a: streak length\nframe { back = 8 + a*240 }\ndraw {\n  for k < 192 {\n    sx = (hash(k, 7) - 0.5)*4096; sy = (hash(k, 3) - 0.5)*1024  -- where it is, across and up\n    z = frac(hash(k, 11) - t/2.4)*4096        -- how far: 0 near, 4096 far\n    s = 2/(z/16 + 4); st = 2/(min(z + back, 4095)/16 + 4)  -- perspective, now and before\n    lit = 1 - z/4096                          -- fading in from the dark\n    line(64 + sx*st, 16 + sy*st, 64 + sx*s, 16 + sy*s, lit*0.2, lit)\n  }\n}\npal(ice, ink)";
pub const CUBE: &str = "loop 4.8\n-- Wireframe cube: turning about two axes and swinging across; the near\n-- edges brighter. a: size. The colour is the last line's #40c0ff\ntable V = -1,-1,-1, 1,-1,-1, 1,1,-1, -1,1,-1, -1,-1,1, 1,-1,1, 1,1,1, -1,1,1\ntable E = 0,1, 1,2, 2,3, 3,0, 4,5, 5,6, 6,7, 7,4, 0,4, 1,5, 2,6, 3,7\nframe {\n  sa = sin(t/4.8); ca = cos(t/4.8); sb = sin(t/4.8 + 1/6); cb = cos(t/4.8 + 1/6)\n  size = 20 + a*28; mid = 64 + sa*40\n}\ndraw {\n  for e < 12 {\n    m = E[e*2]*3; n = E[e*2 + 1]*3            -- the edge's two corners\n    x0 = V[m]*size; y0 = V[m + 1]*size; z0 = V[m + 2]*size\n    x1 = V[n]*size; y1 = V[n + 1]*size; z1 = V[n + 2]*size\n    xa = x0*ca + z0*sa; za = z0*ca - x0*sa; ya = y0*cb - za*sb; zb = y0*sb + za*cb  -- turned\n    xc = x1*ca + z1*sa; zc = z1*ca - x1*sa; yc = y1*cb - zc*sb; zd = y1*sb + zc*cb\n    line(mid + xa*36/(zb + 128), 16 + ya*36/(zb + 128), mid + xc*36/(zd + 128), 16 + yc*36/(zd + 128),\n         clamp(0.6 - zb/90, 0.16, 1), clamp(0.6 - zd/90, 0.16, 1))\n  }\n}\n#40c0ff * ink";

// psi-speed3 (psi-probe on the spare, 2026-10-28, 1.0.0-beta.11-s3probe-a;
// s2probe-b's before it): the draw rows with their loops run as lanes, VM
// p50 µs a frame, compared over the one-dot base (`DRAW_LANES_BASE`); the
// last two rows: steps laned since psi-speed3, and steps still worked out
// turn by turn (`LANE_EVAL_NS`).
pub const DRAW_LANES_BASE: u32 = 633;
pub const DRAW_LANES: &[(&str, &str, u32)] = &[
    (
        "lanes: 1024 dots off",
        "draw { for k < 1024 { dot(-1, -1, t) } }\ngray(ink)",
        1371,
    ),
    (
        "lanes: +add",
        "draw { for k < 1024 { dot(-1, -1, k + t) } }\ngray(ink)",
        1516,
    ),
    (
        "lanes: +hash",
        "draw { for k < 1024 { dot(-1, -1, hash(k, t)) } }\ngray(ink)",
        1597,
    ),
    (
        "lanes: dot on panel x1024",
        "draw { for k < 1024 { dot(k % 128, 5 + t) } }\ngray(ink)",
        2044,
    ),
    (
        "lanes: line 12 x256",
        "draw { for k < 256 { line(10, 5, 21, 5 + t) } }\ngray(ink)",
        1882,
    ),
    (
        "lanes: line 100 x64",
        "draw { for k < 64 { line(10, 5, 109, 5 + t) } }\ngray(ink)",
        2395,
    ),
    (
        "lanes: laned mod cmp sin",
        "draw { for k < 1024 { dot(-1, -1, sin(k*0.01 + t) + (k % 7 >= 3) + (k % 5 != 1)) } }\ngray(ink)",
        2236,
    ),
    (
        "lanes: eval noise digit sqrt step",
        "draw { for k < 1024 { dot(-1, -1, noise(k*0.1, t, 0.5) + digit(k % 10, 1, 2) + sqrt(k + t) + step(0.5, frac(k*0.3))) } }\ngray(ink)",
        5903,
    ),
];
// The examples' VM frames (`speed2` rows, as compiled; with the still cache
// where it applies; slider a = 0.5): Starfield from s2probe-b, the cube
// its steps' row there (its colour of ink is no longer fused), the rest from s2probe-a (which had the multiply-add fused, as
// now; s2probe-b had not).
pub const SPEED2: &[(&str, &str, u32)] = &[
    ("Starfield", "loop 2.4\nfps 100\n-- Starfield: 192 stars flying at you, a pass through the field a loop,\n-- each a streak from where it was a moment ago\n-- a: streak length\nframe { back = 1 + 1/64 + (144 + a*240)/4096 }\ndraw {\n  for k < 192 {\n    sx = hash(k, 7)*32 - 16; sy = hash(k, 3)*8 - 4  -- where it is, across and up\n    near = frac(t/2.4 - hash(k, 11))          -- 0 far, 1 at you\n    s = 1/(65/64 - near); st = 1/(back - near)  -- perspective, now and a moment ago\n    hx = sx*s; hy = sy*s                      -- the head, from the middle\n    -- a head off the panel: the line moved far below it, where it is skipped\n    row = abs(hx) > 64 or abs(hy) > 16 ? -999 : 16\n    lit = near*near                           -- squared: far stars stay dark longer\n    line(64 + sx*st, row + sy*st, 64 + hx, row + hy, lit*0.2, lit)\n  }\n}\npal(ice, ink)", 2078),
    ("Wireframe cube", "loop 4.8\nfps 100\n-- Wireframe cube: turning about two axes and swinging across; the near\n-- edges brighter. The colour is the last line's #40c0ff\n-- a: size\ntable V = -1,-1,-1, 1,-1,-1, 1,1,-1, -1,1,-1, -1,-1,1, 1,-1,1, 1,1,1, -1,1,1\ntable E = 0,1, 1,2, 2,3, 3,0, 4,5, 5,6, 6,7, 7,4, 0,4, 1,5, 2,6, 3,7\nframe {\n  sa = sin(t/4.8); ca = cos(t/4.8); sb = sin(t/4.8 + 1/6); cb = cos(t/4.8 + 1/6)\n  size = 34 + a*28; mid = 64 + sa*40\n}\ndraw {\n  for e < 12 {\n    m = E[e*2]*3; n = E[e*2 + 1]*3            -- the edge's two corners\n    x0 = V[m]*size; y0 = V[m + 1]*size; z0 = V[m + 2]*size\n    x1 = V[n]*size; y1 = V[n + 1]*size; z1 = V[n + 2]*size\n    xa = x0*ca + z0*sa; za = z0*ca - x0*sa; ya = y0*cb - za*sb; zb = y0*sb + za*cb  -- turned\n    xc = x1*ca + z1*sa; zc = z1*ca - x1*sa; yc = y1*cb - zc*sb; zd = y1*sb + zc*cb\n    line(mid + xa*36/(zb + 128), 16 + ya*36/(zb + 128), mid + xc*36/(zd + 128), 16 + yc*36/(zd + 128),\n         clamp(0.594 - zb/148, 0.156, 1), clamp(0.594 - zd/148, 0.156, 1))\n  }\n}\n#40c0ff * ink", 2183),
    ("Tunnel", "loop 4.8\nfps 100\n-- Tunnel: the time warp. Each loop the walls turn once, travel two\n-- checker lengths, and the palette goes round once.\n-- From the vanishing point, the tunnel twice as tall as wide:\ndx = px - 63.5; dy = (15.5 - py)*2\n-- how far each pixel is from the black middle: 0 there (where the\n-- tunnel is deeper than 16), ~15 at the edges\n-- (this and `round` depend on the pixel alone: worked out once, kept)\nnear = 16 - min(128/len(dx, dy), 16)\n-- round the walls: 16 checker cells a turn, twisting with depth\ntwist = 1                              -- try 0 to 3\nround = atan2(dy, dx)*16 + (16 - near)*twist/8\n-- the checker, turning and travelling\nu = round + t/0.3; v = 16 + t/0.6 - near\ncell = (floor(u) + floor(v)) % 2\n-- lit by depth (full near, fading as a square, black in the middle) and\n-- by a light going round with the walls, in eight steps\nfade = min(near, 10)                    -- 10 near, to 0 where it turns black\nside = 0.0635 + 0.0159*cos(u/16)\nlight = floor(fade*fade*side + (near > 0))/8\npal(chrono, cell ? frac(0.711 + t/4.8) : frac(0.336 + t/4.8)) * light", 3208),
    ("Rotozoom", "loop 9.6\n-- Rotozoom: Mode 7. A checker turning once a loop, its zoom breathing\n-- twice, gliding two checkers across and one down; under it a noise\n-- texture turning the other way twice as fast; rows and columns wobble.\n-- a: wobble = gentle, strong, off\nframe {\n  sn = sin(t/9.6); cs = cos(t/9.6)                 -- the turn\n  s2 = -sin(t/4.8); c2 = cos(t/4.8)                -- the noise's, back twice\n  zoom = 2                                         -- try 1 to 4\n  sc = 1/((1 + sin(t/4.8)/2)*zoom*8)               -- checker cells a pixel\n  sc2 = sc*0.75\n  cu = t/0.6; cv = t/1.2                           -- the glide, in cells\n  amp = pick(floor(a*2 + 0.5), 3, 4.5, 0)/8        -- the wobble, in texels\n}\ndx = px - 64; dy = 15 - py\n-- the wobble: rows slide sideways, columns up and down, on travelling waves\nwob = sin((31 - py)/32 + t/3.2)*amp\ncol = sin(px*0.0234375 - t/4.8)*amp\n-- where each pixel is on the checker: its column's part plus its row's\nu = (cu + cs*sc*dx) + (wob - sn*sc*dy)\nv = (cv + sn*sc*dx + col) + cs*sc*dy\ncell = (floor(u) + floor(v)) % 2\nu2 = (cv + c2*sc2*dx) + (wob - s2*sc2*dy)\nv2 = (cu + s2*sc2*dx + col) + c2*sc2*dy\npal(fire, (cell ? 0.44 : 0.19) + noise(u2, v2, 0)*0.6)", 4309),
    ("Rotozoom floor", "loop 9.6\nfps 100\n-- Rotozoom floor: Mode 7 as a floor. A checker running away to a horizon\n-- at the top, turning once a loop and gliding, each row scaled by its\n-- distance and the far rows fogged; rows and columns wobble.\n-- a: wobble = gentle, strong, off\nframe {\n  sn = sin(t/9.6); cs = cos(t/9.6)                 -- the turn\n  zoom = 2                                         -- try 1 to 4\n  cu = t/0.6; cv = t/1.2                           -- the glide, in cells\n  amp = pick(floor(a*2 + 0.5), 3, 4.5, 0)/8        -- the wobble, in texels\n}\ndx = px - 64; ty = 31 - py\n-- a row's distance: checker cells a pixel, smaller further down\nrs = 1/((ty + 2)*zoom)\nwob = sin(ty/32 + t/3.2)*amp\ncol = sin(px*0.0234375 - t/4.8)*amp\n-- ahead by the row's distance, across by the pixel's\nu = cs*rs*dx + (cu + wob + sn*rs*64)\nv = sn*rs*dx + col + (cv - cs*rs*64)\ncell = (floor(u) + floor(v)) % 2\n-- the far rows fade into the fog (black at the horizon)\nfog = ty < 3 ? 0 : ty < 6 ? 0.375 : ty < 10 ? 0.625 : 1\npal(fire, cell ? 0.78 : 0.28) * fog", 1618),
    ("Endless road", "loop forever\nfps 100\n-- Endless road: an OutRun drive into a synthwave sunset. Bends and hills\n-- come from noise along the distance driven, which grows without end, so\n-- the track doesn't repeat; each time it plays it starts somewhere else.\n-- The car, from behind: red body (6), black glass and tyres (7), orange\n-- tail lights (8): the colours' places in the pick at the end\nsprite car {\n  key . 0, R 6, D 7, L 8\n  ........RRRRRRRRRR........\n  .......RDDDDDDDDDDR.......\n  ......RDDDDDDDDDDDDR......\n  ...RRRRRRRRRRRRRRRRRRRR...\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RLLLLRDDDDDDDDDDDDRLLLLR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  .RRRRRRRRRRRRRRRRRRRRRRRR.\n  DDDDDDDDDDDDDDDDDDDDDDDDDD\n}\n-- The mountains: 19 spans (row, from, to)\ntable MY = 19,19,19,20,20,20,21,21,21,22,22,22,23,23,23,24,24,25,26\ntable MA = -1,57,118,0,59,120,2,61,122,4,62,123,5,64,125,7,66,67,69\ntable MB = 17,83,134,16,81,132,14,79,130,12,78,129,11,76,127,9,74,73,71\n-- Road rows py 0 to 18: depth/2 in bends (ZH), depth in stripes (ZS),\n-- half width in pixels (HW); the sun's rows, half widths and colours\ntable ZH = .0058,.0061,.0064,.0068,.0072,.0077,.0083,.009,.0098,.011,.012,.013,.015,.017,.02,.025,.032,.045,.075\ntable ZS = .77,.81,.86,.91,.97,1.03,1.11,1.2,1.3,1.43,1.58,1.76,2,2.31,2.73,3.33,4.29,6,10\ntable HW = 50.7,48.1,45.5,42.9,40.3,37.7,35.1,32.5,29.9,27.3,24.7,22.1,19.5,16.9,14.3,11.7,9.1,6.5,3.9\ntable UY = 20,21,23,24,25,26,27,28,29,30,31\ntable UC = 7.2,7.6,8,8,7.9,7.6,7.2,6.7,6,5,3.5\ntable UI = .344,.344,.281,.281,.281,.281,.219,.219,.219,.219,.219\nframe {\n  u = t*0.15 + seed*1000                    -- distance driven, in bends\n  h0 = noise(u, 0, 0); h1 = noise(u + 0.03, 0, 0)    -- heading here, just ahead\n  v0 = noise(u, 5, 0)                       -- height here, and its slope\n  dv = (noise(u + 0.01, 5, 0) - v0)*100\n  cx = 50.5 + (h1 - h0)*80                  -- the car's left edge: it slides out in bends\n  mx = (0.5 - h0)*60; sunx = 64 + mx        -- the sky turns with the road\n}\ndraw {\n  for k < 19 {\n    my = MY[k]\n    line(MA[k] + mx, my, MB[k] + mx, my, 0.46875)\n  }\n  for k < 11 {\n    uy = UY[k]; uc = UC[k]\n    line(sunx - uc, uy, sunx + uc, uy, UI[k])\n  }\n}\n-- each row looks ahead: how far the road bends there, and how far a hill\n-- lifts or hides it (a crest moves rows to where the road is out of sight)\nzp = ZH[py]\nun = u + zp\nq = py - clamp((noise(un, 5, 0) - v0 - zp*dv)*ZS[py]*200, -6, 2)\nhw = HW[q]\nalt = frac(t*10 + ZS[q]) < 0.5             -- light or dark stripe: 10 a second\nadx = abs(px - (64 + (noise(un, 0, 0) - h0)*2000))\n-- 0 road, 1 kerb, 2 verge or sky; drawn: 3-5 sun, 7 mountains; 6-8 the car\nm = max(max((adx >= hw) + (adx >= hw*1.2), ink*16), car[px - cx, ty - 23])\nlow = py < 19\n-- colours: entries of the rainbow palette (entry k is k/31)\npal(rainbow, pick(m, alt ? 30/31 : 1,\n                     alt ? 4/31 : 10/31,\n                     low ? (alt ? 2/31 : 1/31) : 1.17 - py*0.0161,\n                     10/31, 7/31, 5/31, 3/31, 0, 6/31))", 3733),
    ("Plasma", "loop 8\n-- three waves added together\nwave = sin(x*2 + t/8) + sin(y*1.5 - t/4) + sin(x*1.5 + y + t/8)\nhsv(wave/6 + t/8, 0.9, 1)", 2799),
    ("Fire", "loop 4\n-- curl bends the flames, more and more as they rise\ncurl = sin(y*1.5 - t)*0.25 + sin(y*3 - t*2 + x*4)*0.15\nflames = sin(x*6 + curl)*0.5 + sin(x*11 - curl)*0.5\nheat = (1 - y)*(1.6 + flames*0.7) - 0.5\npal(fire, clamp(heat, 0, 0.95))", 3753),
    // roadprobe-c (2026-10-29, the spare, once-a-row helpers in IRAM): the
    // VM frame of Endless road as bundled until road-cost (the sprite car;
    // its split: setup 234 + rows 3,443) and with its car drawn as 17 spans
    // (the same bytes; its effect frame 3,393 less the 42 µs the effect
    // path adds to the sprite one's). In play the governor read ~3,970 µs
    // for the first: the panel and the network share the flash cache.
    ("Endless road (sprite car)", ROAD_SPRITE, 3677),
    ("Endless road (car drawn)", ROAD_DRAWN, 3351),
];

// The JIT on the spare (psi-probe JIT2_ONLY, 1.0.0-beta.11-jit2-c,
// 2026-10-29): each bundled formula as an effect on the player's path with
// the JIT on (its kernel, after the self-check and the trial; the still
// cache taken where it has values), p50 µs a frame; and the looks over a
// changing picture. For `Program::esp32_us_jit_still` / `esp32_us_jit`.
pub const JIT_EFFECTS: &[(&str, &str, u32)] = &[
    // The road as bundled from road-life to road-full (Both), and Full as
    // measured: the bundled road since road-full is Full respelled for its
    // `about` (the same frames, two frame steps fewer), not measured itself.
    ("Endless road Both", include_str!("../../opendmd-web/experiments/road-life/road-both.psi"), 2723),
    ("Endless road Full", include_str!("../../opendmd-web/experiments/road-life/road-full.psi"), 3601),
    ("Wireframe cube", include_str!("../../opendmd-web/formulas/wireframe-cube.psi"), 1691),
    ("Plasma letters", include_str!("../../opendmd-web/formulas/plasma-letters.psi"), 4793),
    ("Rotozoom floor", include_str!("../../opendmd-web/formulas/rotozoom-floor.psi"), 1517),
    ("Tunnel", include_str!("../../opendmd-web/formulas/tunnel.psi"), 2520),
    ("Time gate", include_str!("../../opendmd-web/formulas/time-gate.psi"), 2816),
    ("Fire", include_str!("../../opendmd-web/formulas/fire.psi"), 2392),
    ("Plasma", include_str!("../../opendmd-web/formulas/plasma.psi"), 2657),
];
pub const JIT_LOOKS: &[(&str, &str, &str, u8, u32)] = &[
    ("Old TV", include_str!("../../opendmd-web/formulas/old-tv.psi"), "over", 100, 1873),
    ("Plasma letters", include_str!("../../opendmd-web/formulas/plasma-letters.psi"), "over", 100, 4895),
    ("Plasma", include_str!("../../opendmd-web/formulas/plasma.psi"), "screen", 40, 4083),
];
