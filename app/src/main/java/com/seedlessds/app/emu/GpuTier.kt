package com.seedlessds.app.emu

/**
 * Recognises GPUs that struggle with the multi-pass presentation shaders.
 *
 * SeedlessDS rasterises 3D on the CPU, so the GPU only runs the shader chain that
 * scales the two screens. The default filter ("None") is already one pass with
 * one nearest tap. FXAA, FXAA HQ, SMAA and HQ2X run two to four passes, and their
 * intermediate passes run at the source resolution, which grows with the internal
 * resolution setting.
 *
 * "Auto-lite" swaps those chains for one bilinear tap on GPUs below Adreno 620.
 * It never touches a filter that is already cheap, and it can be turned off.
 */
object GpuTier {
    /** Multi-pass chains. */
    val HEAVY_FILTERS: Set<String> = setOf("FXAA", "FXAA HQ", "SMAA", "HQ2X")

    /** One pass, one bilinear tap, no anti-aliasing (Linear.dfx). */
    const val LITE_FILTER = "Linear"

    /** Adreno models below this number are treated as weak. */
    const val ADRENO_STRONG_FROM = 620

    private val adreno = Regex("""Adreno\D*(\d{3,4})""", RegexOption.IGNORE_CASE)

    @Volatile
    var renderer: String = ""
        private set

    @Volatile
    var weak: Boolean = false
        private set

    /** The model number of an Adreno GL_RENDERER string such as "Adreno (TM) 618", or null. */
    fun adrenoModel(name: String): Int? = adreno.find(name)?.groupValues?.get(1)?.toIntOrNull()

    fun isWeak(name: String): Boolean {
        val model = adrenoModel(name) ?: return false
        return model < ADRENO_STRONG_FROM
    }

    /** Call once the GL context is current, with GL_RENDERER. */
    fun detect(glRenderer: String) {
        renderer = glRenderer
        weak = isWeak(glRenderer)
    }

    fun effectiveFilter(filter: String, autoLite: Boolean): String =
        if (autoLite && weak && filter in HEAVY_FILTERS) LITE_FILTER else filter

    /** True when [effectiveFilter] would replace the user's choice. */
    fun liteActive(filter: String, autoLite: Boolean): Boolean = effectiveFilter(filter, autoLite) != filter
}
