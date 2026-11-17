package com.seedlessds.app.emu

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowLeft
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Mirror of the buttons pressed on an EXTERNAL controller (physical keys, d-pad/hat, stick and
 * analog triggers). Bit positions are the DS button indexes used by [EmulatorActivity]
 * (UP=0 .. SELECT=11). On-screen touch buttons never write here, so the display only reflects
 * the real controller. Written from the main thread only; Compose observes [mask].
 */
object ExternalPadState {
    var mask by mutableStateOf(0)
        private set

    fun set(index: Int, pressed: Boolean) {
        if (index < 0) return
        val bit = 1 shl index
        val next = if (pressed) mask or bit else mask and bit.inv()
        if (next != mask) mask = next
    }

    fun clear() { if (mask != 0) mask = 0 }
}

private val PressedColor = SeedlessColors.gold
private val IdleColor = Color(0x66000000)
private val IdleBorder = Color(0x88FFFFFF)

/**
 * Compact controller diagram: L/R on top, d-pad, SELECT/START, face buttons. Size and position are
 * controlled by the caller (ControlsOverlay applies the layout editor's offset and scale).
 */
@Composable
fun InputDisplay(modifier: Modifier = Modifier, mask: Int, opacity: Float = 0.9f) {
    fun down(index: Int) = mask and (1 shl index) != 0
    val C = EmulatorActivity

    Column(
        modifier.alpha(opacity.coerceIn(0.2f, 1f)),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        Row(Modifier.width(214.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            PadKey(Modifier.size(60.dp, 20.dp), RoundedCornerShape(8.dp), down(C.L)) { KeyLabel("L") }
            PadKey(Modifier.size(60.dp, 20.dp), RoundedCornerShape(8.dp), down(C.R)) { KeyLabel("R") }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.size(70.dp)) {
                PadKey(Modifier.size(24.dp).align(Alignment.TopCenter), RoundedCornerShape(5.dp), down(C.UP)) { Arrow(Icons.Filled.KeyboardArrowUp, down(C.UP)) }
                PadKey(Modifier.size(24.dp).align(Alignment.BottomCenter), RoundedCornerShape(5.dp), down(C.DOWN)) { Arrow(Icons.Filled.KeyboardArrowDown, down(C.DOWN)) }
                PadKey(Modifier.size(24.dp).align(Alignment.CenterStart), RoundedCornerShape(5.dp), down(C.LEFT)) { Arrow(Icons.Filled.KeyboardArrowLeft, down(C.LEFT)) }
                PadKey(Modifier.size(24.dp).align(Alignment.CenterEnd), RoundedCornerShape(5.dp), down(C.RIGHT)) { Arrow(Icons.Filled.KeyboardArrowRight, down(C.RIGHT)) }
            }
            Column(verticalArrangement = Arrangement.spacedBy(6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                PadKey(Modifier.size(52.dp, 16.dp), RoundedCornerShape(8.dp), down(C.SELECT)) { KeyLabel("SEL", 8) }
                PadKey(Modifier.size(52.dp, 16.dp), RoundedCornerShape(8.dp), down(C.START)) { KeyLabel("STA", 8) }
            }
            Box(Modifier.size(70.dp)) {
                PadKey(Modifier.size(24.dp).align(Alignment.TopCenter), CircleShape, down(C.X)) { KeyLabel("X") }
                PadKey(Modifier.size(24.dp).align(Alignment.BottomCenter), CircleShape, down(C.B)) { KeyLabel("B") }
                PadKey(Modifier.size(24.dp).align(Alignment.CenterStart), CircleShape, down(C.Y)) { KeyLabel("Y") }
                PadKey(Modifier.size(24.dp).align(Alignment.CenterEnd), CircleShape, down(C.A)) { KeyLabel("A") }
            }
        }
    }
}

@Composable
private fun PadKey(modifier: Modifier, shape: Shape, pressed: Boolean, content: @Composable () -> Unit) {
    Box(
        modifier
            .background(if (pressed) PressedColor else IdleColor, shape)
            .border(BorderStroke(1.dp, if (pressed) PressedColor else IdleBorder), shape),
        contentAlignment = Alignment.Center
    ) { content() }
}

@Composable
private fun KeyLabel(text: String, size: Int = 11) {
    Text(text, color = Color.White, fontSize = size.sp, fontWeight = FontWeight.Bold, maxLines = 1)
}

@Composable
private fun Arrow(icon: androidx.compose.ui.graphics.vector.ImageVector, pressed: Boolean) {
    Icon(icon, contentDescription = null, tint = if (pressed) Color.Black else Color.White, modifier = Modifier.size(20.dp).padding(0.dp))
}
