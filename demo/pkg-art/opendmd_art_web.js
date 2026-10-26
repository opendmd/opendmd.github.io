/* @ts-self-types="./opendmd_art_web.d.ts" */

/**
 * Smooth motion for one GIF (`opendmd_core::tween::Smooth`), in steps so
 * the page can yield between them: `new Smoother(gifBytes)`, then
 * `analyse(n)` until it returns true (`n` frames a call; `decoded` counts
 * them), then the analysis (`frames`, `pairs`, `tweened`, `added`,
 * `percent`, `offered`, `hint`); `tween(n)` until true, then `gif()` — the tweened
 * frames as a plain GIF, what the preview plays — and `prepare()`, the
 * smoothed prepared copy (marked). Throws on a GIF that does not decode.
 */
export class Smoother {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SmootherFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_smoother_free(ptr, 0);
    }
    /**
     * Frames Smooth motion adds.
     * @returns {number}
     */
    get added() {
        const ret = wasm.smoother_added(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Decode and classify up to `frames` more frames; true once done.
     * @param {number} frames
     * @returns {boolean}
     */
    analyse(frames) {
        const ret = wasm.smoother_analyse(this.__wbg_ptr, frames);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * Frames decoded so far (the total once analysed).
     * @returns {number}
     */
    get decoded() {
        const ret = wasm.smoother_decoded(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * At most 256 colours, so a smoothed copy can be made at all (past
     * 256 the GIF plays as it is, never smoothed).
     * @returns {boolean}
     */
    get fits() {
        const ret = wasm.smoother_fits(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    get frames() {
        const ret = wasm.smoother_frames(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The tweened frames as a plain GIF (empty until `tween` is done).
     * @returns {Uint8Array}
     */
    gif() {
        const ret = wasm.smoother_gif(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Why there is little or nothing to smooth, for the wording:
     * `"offered"`, `"steady"` (it moves 1 px at a time: already as smooth
     * as whole pixels allow), `"fast"` (big steps in frames too short to
     * split) or `"nothing"` (nothing slides).
     * @returns {string}
     */
    get hint() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.smoother_hint(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {Uint8Array} gif
     */
    constructor(gif) {
        const ptr0 = passArray8ToWasm0(gif, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.smoother_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        SmootherFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Worth offering (≥ 10% of the pairs).
     * @returns {boolean}
     */
    get offered() {
        const ret = wasm.smoother_offered(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Frame pairs, the loop's last -> first included.
     * @returns {number}
     */
    get pairs() {
        const ret = wasm.smoother_pairs(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * `tweened` as a whole percentage of `pairs`, rounded down.
     * @returns {number}
     */
    get percent() {
        const ret = wasm.smoother_percent(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The smoothed prepared copy: the tweened GIF through the recoder in
     * the prepared format, marked `OPENDMD1TWN`.
     * @returns {Uint8Array}
     */
    prepare() {
        const ret = wasm.smoother_prepare(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Write up to `frames` more source frames of the tweened GIF; true
     * once done.
     * @param {number} frames
     * @returns {boolean}
     */
    tween(frames) {
        const ret = wasm.smoother_tween(this.__wbg_ptr, frames);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * Pairs that move in steps of 2 px or more the tweener takes.
     * @returns {number}
     */
    get tweened() {
        const ret = wasm.smoother_tweened(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) Smoother.prototype[Symbol.dispose] = Smoother.prototype.free;

/**
 * One picture being made into art. `new Studio(rgba, width, height)` from
 * `ImageData`; then `prepare(fit, lift)` builds the stage (the picture the
 * window moves over, `width` x `height`, `pixels()`), `autoMotion()` and
 * `autoWindow(motion)` suggest, and `render(...)` makes the GIF.
 *
 * Or an animated GIF: `Studio.fromGif(bytes)`, then `decode(n)` until true
 * (the frames, `n` a call, so the page can yield), `prepare` as above (the
 * stage is the first frame's), `look(...)` for the Frame and Look steps,
 * `setEdit(...)` for the timeline, `frameGif(i)` / `thumb(i)` per source
 * frame (rendered when first asked for), and `saveBegin(plays)`,
 * `saveStep(n)` until true, `saved()` for the GIF ([`anim`]).
 */
export class Studio {
    static __wrap(ptr) {
        const obj = Object.create(Studio.prototype);
        obj.__wbg_ptr = ptr;
        StudioFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        StudioFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_studio_free(ptr, 0);
    }
    /**
     * @returns {boolean}
     */
    get animated() {
        const ret = wasm.studio_animated(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * `"pixels"`, `"half"` or `"fit"`: what to start with.
     * @returns {string}
     */
    autoFit() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.studio_autoFit(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * `"still"`, `"pan"` or `"scroll"`, from the stage's shape.
     * @returns {string}
     */
    autoMotion() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.studio_autoMotion(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * `[x, y]`: where the window starts on the stage for a motion
     * (`"still"`: the busiest 128x32; `"pan"`: the busiest band;
     * `"scroll"`: the busiest column).
     * @param {string} motion
     * @returns {Uint32Array}
     */
    autoWindow(motion) {
        const ptr0 = passStringToWasm0(motion, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_autoWindow(this.__wbg_ptr, ptr0, len0);
        var v2 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * @returns {number}
     */
    get colours() {
        const ret = wasm.studio_colours(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Decode up to `n` more frames; true once all are in.
     * @param {number} n
     * @returns {boolean}
     */
    decode(n) {
        const ret = wasm.studio_decode(this.__wbg_ptr, n);
        return ret !== 0;
    }
    /**
     * Source frame delays as the player shows them, ms.
     * @returns {Uint32Array}
     */
    delays() {
        const ret = wasm.studio_delays(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {number}
     */
    get durationMs() {
        const ret = wasm.studio_durationMs(this.__wbg_ptr);
        return ret;
    }
    /**
     * Source frame `i`, edited, as a one-frame GIF for the preview.
     * @param {number} i
     * @returns {Uint8Array}
     */
    frameGif(i) {
        const ret = wasm.studio_frameGif(this.__wbg_ptr, i);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Of the last `render`.
     * @returns {number}
     */
    get frames() {
        const ret = wasm.studio_frames(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * A live effect's path (`@fx:tunnel?palette=fire…`) as an animation:
     * one loop of it, drawn by the core's effect ([`anim::effect_plan`]
     * says how many frames at what delay); `decode` draws the rest. Throws
     * if the path names no effect.
     * @param {string} path
     * @returns {Studio}
     */
    static fromEffect(path) {
        const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_fromEffect(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return Studio.__wrap(ret[0]);
    }
    /**
     * Frames drawn by the page — a formula's, by the Psi module: `n`
     * frames of 128x32 RGB888 at `delayMs` each, `first` now and the rest
     * by `push` (`cut`: they are not a whole loop). Throws on a frame of
     * the wrong size.
     * @param {Uint8Array} first
     * @param {number} n
     * @param {number} delay_ms
     * @param {boolean} cut
     * @returns {Studio}
     */
    static fromFrames(first, n, delay_ms, cut) {
        const ptr0 = passArray8ToWasm0(first, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_fromFrames(ptr0, len0, n, delay_ms, cut);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return Studio.__wrap(ret[0]);
    }
    /**
     * An animated GIF: its header and first frame read (throws if it does
     * not decode); `decode` reads the rest.
     * @param {Uint8Array} bytes
     * @returns {Studio}
     */
    static fromGif(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_fromGif(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return Studio.__wrap(ret[0]);
    }
    /**
     * @returns {number}
     */
    get height() {
        const ret = wasm.studio_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Frames the file has (read before decoding them).
     * @returns {number}
     */
    get inFile() {
        const ret = wasm.studio_inFile(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * `[width, height, workWidth, workHeight, reduce, damaged, capped, plays,
     * cut]` of an animation (plays: 0 forever; cut: an effect longer than
     * the frames it was given); empty for a still.
     * @returns {Uint32Array}
     */
    info() {
        const ret = wasm.studio_info(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Whether `thumb(i)` is ready without work.
     * @param {number} i
     * @returns {boolean}
     */
    isRendered(i) {
        const ret = wasm.studio_isRendered(this.__wbg_ptr, i);
        return ret !== 0;
    }
    /**
     * An animation's Frame and Look steps (the window at `x`, `y`; the
     * rest as `render`). Rendered frames are kept if nothing changed.
     * @param {number} x
     * @param {number} y
     * @param {boolean} key
     * @param {number} gain
     * @param {number} colours
     */
    look(x, y, key, gain, colours) {
        wasm.studio_look(this.__wbg_ptr, x, y, key, gain, colours);
    }
    /**
     * @param {Uint8Array} rgba
     * @param {number} width
     * @param {number} height
     */
    constructor(rgba, width, height) {
        const ptr0 = passArray8ToWasm0(rgba, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_new(ptr0, len0, width, height);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        StudioFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * The stage as RGBA bytes, for drawing.
     * @returns {Uint8Array}
     */
    pixels() {
        const ret = wasm.studio_pixels(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Build the stage for a fit (`"pixels"`, `"half"`, `"fit"`), lifting
     * dark strokes first when asked.
     * @param {string} fit
     * @param {boolean} lift
     */
    prepare(fit, lift) {
        const ptr0 = passStringToWasm0(fit, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.studio_prepare(this.__wbg_ptr, ptr0, len0, lift);
    }
    /**
     * The next of `fromFrames`' frames; true once all are in.
     * @param {Uint8Array} rgb
     * @returns {boolean}
     */
    push(rgb) {
        const ptr0 = passArray8ToWasm0(rgb, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_push(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * The GIF: `motion` `"still"`/`"pan"`/`"scroll"` from window (`x`, `y`),
     * `step_ms` a pixel, `hold_ms` on the end frames, background to black,
     * brightness `gain` percent, at most `colours` colours (0: auto).
     * @param {string} motion
     * @param {number} x
     * @param {number} y
     * @param {number} step_ms
     * @param {number} hold_ms
     * @param {boolean} key
     * @param {number} gain
     * @param {number} colours
     * @returns {Uint8Array}
     */
    render(motion, x, y, step_ms, hold_ms, key, gain, colours) {
        const ptr0 = passStringToWasm0(motion, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_render(this.__wbg_ptr, ptr0, len0, x, y, step_ms, hold_ms, key, gain, colours);
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * Start writing the edited animation (the last `setEdit`), played
     * `plays` times (0: forever).
     * @param {number} plays
     */
    saveBegin(plays) {
        wasm.studio_saveBegin(this.__wbg_ptr, plays);
    }
    /**
     * `[done, of]`: the save's progress, in frames of work.
     * @returns {Uint32Array}
     */
    saveProgress() {
        const ret = wasm.studio_saveProgress(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `n` frames' worth of the save; true once written.
     * @param {number} n
     * @returns {boolean}
     */
    saveStep(n) {
        const ret = wasm.studio_saveStep(this.__wbg_ptr, n);
        return ret !== 0;
    }
    /**
     * The saved GIF (then `colours` is its palette's size).
     * @returns {Uint8Array}
     */
    saved() {
        const ret = wasm.studio_saved(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The upscale undone by the pixel fits (1: none found).
     * @returns {number}
     */
    get scale() {
        const ret = wasm.studio_scale(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The timeline: source frames `from..=to` less `deleted`, at `speed`
     * percent, reversed, ping-pong ([`pipeline::sequence`]). Returns the
     * play order flattened, `[frame, delay ms, frame, delay ms, ...]`.
     * @param {number} from
     * @param {number} to
     * @param {number} speed
     * @param {boolean} reverse
     * @param {boolean} pingpong
     * @param {Uint32Array} deleted
     * @returns {Uint32Array}
     */
    setEdit(from, to, speed, reverse, pingpong, deleted) {
        const ptr0 = passArray32ToWasm0(deleted, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.studio_setEdit(this.__wbg_ptr, from, to, speed, reverse, pingpong, ptr0, len0);
        var v2 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * Text over the frames: `msg` in the small (4x6) or normal (5x7)
     * font, colour `0xRRGGBB`, anchor 0..=8 (top-left, row by row), nudged
     * by `dx`, `dy`, over source frames `from..=to` (an animation's). An
     * empty message is no text.
     * @param {string} msg
     * @param {boolean} small
     * @param {number} colour
     * @param {number} anchor
     * @param {number} dx
     * @param {number} dy
     * @param {number} from
     * @param {number} to
     */
    setText(msg, small, colour, anchor, dx, dy, from, to) {
        const ptr0 = passStringToWasm0(msg, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.studio_setText(this.__wbg_ptr, ptr0, len0, small, colour, anchor, dx, dy, from, to);
    }
    /**
     * Source frames decoded (all of them once `decode` is done).
     * @returns {number}
     */
    get sourceFrames() {
        const ret = wasm.studio_sourceFrames(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Source frame `i`, edited, as 128x32 RGBA (a thumbnail).
     * @param {number} i
     * @returns {Uint8Array}
     */
    thumb(i) {
        const ret = wasm.studio_thumb(this.__wbg_ptr, i);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {number}
     */
    get width() {
        const ret = wasm.studio_width(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) Studio.prototype[Symbol.dispose] = Studio.prototype.free;

/**
 * The card path for an object of [`name_fields`]' shape (its inverse,
 * `NameFields::path`); throws the reason's code (`title`, `title-char`,
 * `platform`, `category`, `author`, `needs-platform`, `too-long`,
 * `reads-back`) when the fields cannot be written as a name that reads
 * back as the same fields.
 * @param {any} fields
 * @returns {string}
 */
export function buildName(fields) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ret = wasm.buildName(fields);
        var ptr1 = ret[0];
        var len1 = ret[1];
        if (ret[3]) {
            ptr1 = 0; len1 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred2_0 = ptr1;
        deferred2_1 = len1;
        return getStringFromWasm0(ptr1, len1);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * A No-Intro name as a title: `(USA)`-style tags dropped, a trailing
 * article moved to the front (`opendmd_core::follow::clean_game_name`).
 * @param {string} name
 * @returns {string}
 */
export function cleanGameName(name) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.cleanGameName(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * What people call a platform tag (`MD` -> "Mega Drive"); empty if unknown.
 * @param {string} platform
 * @returns {string}
 */
export function consoleName(platform) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(platform, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.consoleName(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * How many frames a GIF has, from its block structure (no decoding): the
 * Studio edits a GIF of two or more as an animation.
 * @param {Uint8Array} bytes
 * @returns {number}
 */
export function gifFrames(bytes) {
    const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.gifFrames(ptr0, len0);
    return ret >>> 0;
}

/**
 * A GIF's metadata as its path carries it (`NameFields::from_path`):
 * `{title, platform, category, ordinal, author, flags}` — absent = `null`,
 * `category` `""` for a GIF directly in `gifs/`.
 * @param {string} path
 * @returns {any}
 */
export function nameFields(path) {
    const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.nameFields(ptr0, len0);
    return ret;
}

/**
 * The platform tags the parser knows, written as a name carries them
 * (`NDS`, not `NINTENDO_DS`).
 * @returns {Array<any>}
 */
export function platforms() {
    const ret = wasm.platforms();
    return ret;
}

/**
 * GIF bytes -> its prepared copy with the core's recoder in the prepared
 * format (the one `opendmd-pack` and the device run, so the output is
 * byte-identical). Returns `{copy: Uint8Array, stats: {width, height,
 * frames, colours, gif_bytes, out_bytes}}`; throws on a GIF the player
 * could not play either, and — with `gifslim::ORIGINAL_REASON`, "more
 * than 256 colours: plays as it is" — on one that gets no copy because
 * only a lossy one could be made (the panel plays the GIF itself).
 * @param {Uint8Array} gif
 * @returns {any}
 */
export function prepareGif(gif) {
    const ptr0 = passArray8ToWasm0(gif, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.prepareGif(ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * The smoothed prepared copy in one call (`opendmd_core::tween::smooth_gif`
 * in the prepared format): what `Smoother` gives step by step.
 * @param {Uint8Array} gif
 * @returns {Uint8Array}
 */
export function prepareSmoothGif(gif) {
    const ptr0 = passArray8ToWasm0(gif, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.prepareSmoothGif(ptr0, len0);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v2;
}

/**
 * A copy's Smooth motion marker from its first bytes (~1 KB is plenty):
 * `{pairs, added}`, or `null` for a plain copy (or too few bytes).
 * @param {Uint8Array} head
 * @returns {any}
 */
export function smoothMarker(head) {
    const ptr0 = passArray8ToWasm0(head, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.smoothMarker(ptr0, len0);
    return ret;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_67e7344beaa85059: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_number_get_1dc732b810cb937c: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_92ab86bb19cbc12f: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_5d9e815e6fdf150f: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_from_a39669ce566077da: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_get_989d0a1309644f2b: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_unchecked_363572bdd397d473: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_isArray_5674713bb7b79043: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_length_4e1adc0d42e23620: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_new_bebc3f4757acf305: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_ffa92086ea89f79c: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_from_slice_4ee02165f9de919e: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_push_bfdf956ba476f65b: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_set_a377297433dfea63: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./opendmd_art_web_bg.js": import0,
    };
}

const SmootherFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_smoother_free(ptr, 1));
const StudioFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_studio_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('opendmd_art_web_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
