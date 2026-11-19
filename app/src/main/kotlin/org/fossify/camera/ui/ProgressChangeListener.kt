package org.fossify.camera.ui

import android.widget.SeekBar

/** A [SeekBar] listener that only reports changes made by the user. */
class ProgressChangeListener(
    private val onChanged: (progress: Int) -> Unit,
) : SeekBar.OnSeekBarChangeListener {

    override fun onProgressChanged(seekBar: SeekBar, progress: Int, fromUser: Boolean) {
        if (fromUser) {
            onChanged(progress)
        }
    }

    override fun onStartTrackingTouch(seekBar: SeekBar) {
        // Nothing to do, the value is applied while dragging.
    }

    override fun onStopTrackingTouch(seekBar: SeekBar) {
        // Nothing to do, the value is applied while dragging.
    }
}
