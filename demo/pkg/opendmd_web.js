/* @ts-self-types="./opendmd_web.d.ts" */

/**
 * A panel-look player bound to one `<canvas>`. The LED scale comes from the
 * canvas element's `width` at construction (1280 wide is scale 10); the
 * renderer lives as long as the object, and [`Preview::load`] swaps the file
 * underneath it.
 */
export class Preview {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PreviewFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_preview_free(ptr, 0);
    }
    /**
     * Stop and blank the panel.
     */
    clear() {
        wasm.preview_clear(this.__wbg_ptr);
    }
    /**
     * @returns {string}
     */
    get error() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.preview_error(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Smoothed milliseconds per rendered frame (render + `putImageData`).
     * @returns {number}
     */
    get frameMs() {
        const ret = wasm.preview_frameMs(this.__wbg_ptr);
        return ret;
    }
    /**
     * One animation-frame step: 0 nothing drawn, 1 a frame, 2 a frame after
     * the dwell ran out (see [`Preview::set_dwell`]), -1 an error (read
     * [`Preview::error`]; the file is dropped).
     * @returns {number}
     */
    frame() {
        const ret = wasm.preview_frame(this.__wbg_ptr);
        return ret;
    }
    /**
     * Play `bytes` (a GIF, or a GIF's prepared copy) as `path`. The bytes are copied
     * into wasm memory once and the previous file is dropped.
     * @param {string} path
     * @param {Uint8Array} bytes
     */
    load(path, bytes) {
        const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        wasm.preview_load(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * @param {HTMLCanvasElement} canvas
     * @param {number} gamma
     * @param {number} gain
     * @param {number} bloom
     * @param {boolean} flat
     */
    constructor(canvas, gamma, gain, bloom, flat) {
        const ret = wasm.preview_new(canvas, gamma, gain, bloom, flat);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PreviewFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {string}
     */
    get path() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.preview_path(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {boolean}
     */
    get paused() {
        const ret = wasm.preview_paused(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Start the file again, transitioning in from what is shown: a
     * preview of the transition, on the page only.
     */
    replay() {
        wasm.preview_replay(this.__wbg_ptr);
    }
    /**
     * The panel's tint and phosphor glow, as `/api/config` names them
     * (`look`: colour, amber, red, green, blue, white; `glow`: off, short,
     * long; unknown = colour / off). Redrawn at once.
     * @param {string} look
     * @param {string} glow
     */
    setDisplay(look, glow) {
        const ptr0 = passStringToWasm0(look, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(glow, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        wasm.preview_setDisplay(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Give files the firmware's dwell (seconds): at least one loop and
     * `min`, at most `max`; then [`Preview::frame`] returns 2. `min <= 0`
     * loops the file forever, which is the default.
     * @param {number} min
     * @param {number} max
     */
    setDwell(min, max) {
        wasm.preview_setDwell(this.__wbg_ptr, min, max);
    }
    /**
     * Slider or checkbox moved: rebuild the renderer and redraw at once, so
     * the change is visible even while paused or between frames.
     * @param {number} gamma
     * @param {number} gain
     * @param {number} bloom
     * @param {boolean} flat
     */
    setLook(gamma, gain, bloom, flat) {
        wasm.preview_setLook(this.__wbg_ptr, gamma, gain, bloom, flat);
    }
    /**
     * @param {boolean} paused
     */
    setPaused(paused) {
        wasm.preview_setPaused(this.__wbg_ptr, paused);
    }
    /**
     * How files come in: `kind` is a transition name (`dissolve`,
     * `wipe_left`, `wipe_right`, `wipe_down`, `wipe_up`, `blinds`, `push`,
     * `fade`, `none`; `wipe` = `wipe_left`) as `/api/config` has it, `ms`
     * its duration (20..2000). An unknown name is Dissolve.
     * @param {string} kind
     * @param {number} ms
     */
    setTransition(kind, ms) {
        const ptr0 = passStringToWasm0(kind, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.preview_setTransition(this.__wbg_ptr, ptr0, len0, ms);
    }
    /**
     * Show one 128 × 32 RGB888 frame (a formula's, drawn by the Psi
     * module) instead of a file, in this preview's look.
     * @param {Uint8Array} rgb
     */
    showRgb(rgb) {
        const ptr0 = passArray8ToWasm0(rgb, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.preview_showRgb(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * The panel was `elapsed_ms` into this file at `at_ms` on this page's
     * `performance.now()` clock, advancing `rate` ms per ms: follow it from
     * now on (see `Player::sync_to`). Returns the drift before correction,
     * positive when the preview was ahead, or NaN with nothing loaded.
     * @param {number} elapsed_ms
     * @param {number} at_ms
     * @param {number} rate
     * @returns {number}
     */
    syncTo(elapsed_ms, at_ms, rate) {
        const ret = wasm.preview_syncTo(this.__wbg_ptr, elapsed_ms, at_ms, rate);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    get synced() {
        const ret = wasm.preview_synced(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * A transition is on screen.
     * @returns {boolean}
     */
    get transitioning() {
        const ret = wasm.preview_transitioning(this.__wbg_ptr);
        return ret !== 0;
    }
}
if (Symbol.dispose) Preview.prototype[Symbol.dispose] = Preview.prototype.free;

/**
 * The bucket directory (`"00"`..`"ff"`) for a prepared copy's file name
 * (the GIF's name) — the FNV-1a rule `opendmd-pack` and the firmware use.
 * @param {string} name
 * @returns {string}
 */
export function bucketFor(name) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bucketFor(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * `/gifs/Consoles/X.gif` -> `/cache/Consoles/<xx>/X.gif` (where the player
 * looks for its prepared copy), or `undefined` for anything that is not a
 * `.gif` under `/gifs/`.
 * @param {string} gif_path
 * @returns {string | undefined}
 */
export function cachePathFor(gif_path) {
    const ptr0 = passStringToWasm0(gif_path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.cachePathFor(ptr0, len0);
    let v2;
    if (ret[0] !== 0) {
        v2 = getStringFromWasm0(ret[0], ret[1]);
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v2;
}

/**
 * Rebuild a card-absolute manifest path (`/gifs/<Category>/<file>.gif`) for
 * a file picked on its own, from whatever the browser gives us.
 *
 * A directory pick yields `webkitRelativePath` — `dmdcard/gifs/Consoles/X.gif`
 * or, if the user picked `gifs/` itself, `gifs/Consoles/X.gif`. A plain file
 * pick yields only the basename. The category matters because `Filter` and
 * `Entry` both key off it, so:
 *
 * 1. a `gifs` segment with at least two segments after it wins;
 * 2. otherwise the file's own parent directory is the category;
 * 3. otherwise `Unknown`.
 * @param {string} relative
 * @returns {string}
 */
export function cardPath(relative) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(relative, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.cardPath(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * The renderer's default bloom, so the page's slider starts where the
 * simulator's `--bloom` does rather than duplicating the constant in JS.
 * @returns {number}
 */
export function defaultBloom() {
    const ret = wasm.defaultBloom();
    return ret;
}

/**
 * The manifest paths passing a filter, as a JS string array. `filter_json`
 * is `{"platforms":[..], "categories":[..], "exclude_categories":[..],
 * "titles":[..] | "title":"..", "exclude_flags":[..], "shuffle":bool,
 * "seed":n, "limit":n}`; every key is optional, lists may also be
 * comma-separated strings, and an empty list is no constraint.
 * @param {string} manifest_text
 * @param {string} filter_json
 * @returns {Array<any>}
 */
export function filterManifest(manifest_text, filter_json) {
    const ptr0 = passStringToWasm0(manifest_text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(filter_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.filterManifest(ptr0, len0, ptr1, len1);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Lowercase alphanumerics — how the core matches titles.
 * @param {string} s
 * @returns {string}
 */
export function normalizeTitle(s) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(s, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.normalizeTitle(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * [`parse_path`] over every non-blank line of a manifest, in one call.
 * @param {string} text
 * @returns {Array<any>}
 */
export function parseManifest(text) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.parseManifest(ptr0, len0);
    return ret;
}

/**
 * A playlist's `.meta` text -> `[{path, never?, w?, dwell?, dwell_x?,
 * loops?}]`, tolerant like the device.
 * @param {string} text
 * @returns {Array<any>}
 */
export function parseOverrides(text) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.parseOverrides(ptr0, len0);
    return ret;
}

/**
 * `Entry::from_path` as a plain object:
 * `{path, category, platform, title, variant, flags}`; absent fields are
 * `null`.
 * @param {string} path
 * @returns {any}
 */
export function parsePath(path) {
    const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.parsePath(ptr0, len0);
    return ret;
}

/**
 * `[{path, ...}]` -> the canonical `.meta` text. Entries that override
 * nothing (or nothing valid) are dropped.
 * @param {Array<any>} list
 * @returns {string}
 */
export function renderOverrides(list) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.renderOverrides(list);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_7a12af2b3f899c5a: function(arg0) {
            const v = arg0;
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_is_undefined_8c687d0b90d5b524: function(arg0) {
            const ret = arg0 === undefined;
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
        __wbg_error_757e9472f8410341: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_from_a39669ce566077da: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_getContext_e0c05ffee530bdcf: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getDate_8e7807f062ade36a: function(arg0) {
            const ret = arg0.getDate();
            return ret;
        },
        __wbg_getDay_4806d3f91116db40: function(arg0) {
            const ret = arg0.getDay();
            return ret;
        },
        __wbg_getFullYear_0cfd5a75cf3d5856: function(arg0) {
            const ret = arg0.getFullYear();
            return ret;
        },
        __wbg_getHours_a9938d9b8972574f: function(arg0) {
            const ret = arg0.getHours();
            return ret;
        },
        __wbg_getMilliseconds_d116526eb8651a6e: function(arg0) {
            const ret = arg0.getMilliseconds();
            return ret;
        },
        __wbg_getMinutes_1fabf71af7d9c8e2: function(arg0) {
            const ret = arg0.getMinutes();
            return ret;
        },
        __wbg_getMonth_01ce17d889ed7e5e: function(arg0) {
            const ret = arg0.getMonth();
            return ret;
        },
        __wbg_getSeconds_44cc4c4b366119da: function(arg0) {
            const ret = arg0.getSeconds();
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
        __wbg_instanceof_CanvasRenderingContext2d_bde5245b14027f3f: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_a3b8566f0a9c5d1a: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
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
        __wbg_new_0_35540e542ba689d2: function() {
            const ret = new Date();
            return ret;
        },
        __wbg_new_227d7c05414eb861: function() {
            const ret = new Error();
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
        __wbg_new_with_u8_clamped_array_and_sh_15a008af56aca047: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = new ImageData(getClampedArrayU8FromWasm0(arg0, arg1), arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_now_0b7736bc17fd7719: function(arg0) {
            const ret = arg0.now();
            return ret;
        },
        __wbg_parse_6937a9050adfb0e1: function() { return handleError(function (arg0, arg1) {
            const ret = JSON.parse(getStringFromWasm0(arg0, arg1));
            return ret;
        }, arguments); },
        __wbg_performance_d96ad0b441f4510a: function(arg0) {
            const ret = arg0.performance;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_push_bfdf956ba476f65b: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_putImageData_581de52c9f40fc59: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.putImageData(arg1, arg2, arg3);
        }, arguments); },
        __wbg_set_a377297433dfea63: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_height_698fb3b255bc1348: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_width_4c3a2252e0dea033: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_stack_3b0d974bbf31e44f: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_8eb4cd83130a11a0: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_1e7044f654e934db: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_d8b50611246a6d92: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_fd0bc376bf0f8b42: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_width_b5e609025d3f7451: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
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
        "./opendmd_web_bg.js": import0,
    };
}

const PreviewFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_preview_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function getClampedArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ClampedArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
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

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

let cachedUint8ClampedArrayMemory0 = null;
function getUint8ClampedArrayMemory0() {
    if (cachedUint8ClampedArrayMemory0 === null || cachedUint8ClampedArrayMemory0.byteLength === 0) {
        cachedUint8ClampedArrayMemory0 = new Uint8ClampedArray(wasm.memory.buffer);
    }
    return cachedUint8ClampedArrayMemory0;
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
    cachedUint8ArrayMemory0 = null;
    cachedUint8ClampedArrayMemory0 = null;
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
        module_or_path = new URL('opendmd_web_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
