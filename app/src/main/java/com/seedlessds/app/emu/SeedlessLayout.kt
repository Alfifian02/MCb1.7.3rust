package com.seedlessds.app.emu

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Modifier
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * One spacing scale for every menu. Portrait and landscape use the same values, so the
 * header, the content and the footer of a screen all sit on the same left and right edge.
 */
object SeedlessSpace {
    val xs: Dp = 4.dp
    val sm: Dp = 8.dp
    val md: Dp = 12.dp

    /** Left and right edge of a screen's header, content and footer. */
    val edge: Dp = 16.dp
    val xl: Dp = 24.dp

    /** Height of the controls in a header or an action strip. */
    val control: Dp = 42.dp

    /**
     * A header action is a 42dp circle holding a 21dp icon, so the icon sits 10dp inside the
     * circle. Trailing padding of [edge] minus this keeps the icon on the content edge.
     */
    val iconInset: Dp = 10.dp
}

enum class WidthClass { COMPACT, REGULAR }

/** Below this width (a phone in portrait) controls are regrouped instead of laid out in one row. */
const val CompactWidthDp = 560

/** Short screens (a phone in landscape) spend less vertical space on chrome. */
const val ShortHeightDp = 480

@Composable
fun rememberWidthClass(): WidthClass =
    if (LocalConfiguration.current.screenWidthDp < CompactWidthDp) WidthClass.COMPACT else WidthClass.REGULAR

@Composable
fun isShortScreen(): Boolean = LocalConfiguration.current.screenHeightDp < ShortHeightDp

/** Vertical padding of header rows: 8dp, or 4dp when the screen is short. */
@Composable
fun headerVerticalPadding(): Dp = if (isShortScreen()) SeedlessSpace.xs else SeedlessSpace.sm

/**
 * Two panels. Regular width: side by side, [start] has a fixed width and [end] takes the rest and
 * receives a `weight(1f)` modifier. Compact width: stacked, with the whole page scrolling, and [end]
 * receives a `fillMaxWidth()` modifier. Do not use `weight` for heights inside [end] in compact mode,
 * because the page is as tall as its content there.
 */
@Composable
fun TwoPane(
    compact: Boolean,
    startWidth: Dp,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(0.dp),
    gap: Dp = SeedlessSpace.edge,
    startSpacing: Dp = SeedlessSpace.sm,
    start: @Composable ColumnScope.() -> Unit,
    end: @Composable (Modifier) -> Unit,
) {
    if (compact) {
        Column(
            modifier.verticalScroll(rememberScrollState()).padding(contentPadding),
            verticalArrangement = Arrangement.spacedBy(gap),
        ) {
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(startSpacing), content = start)
            end(Modifier.fillMaxWidth())
        }
    } else {
        Row(modifier.padding(contentPadding), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Column(Modifier.width(startWidth), verticalArrangement = Arrangement.spacedBy(startSpacing), content = start)
            end(Modifier.weight(1f))
        }
    }
}
