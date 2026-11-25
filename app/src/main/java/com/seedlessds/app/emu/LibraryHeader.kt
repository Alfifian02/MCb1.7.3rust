package com.seedlessds.app.emu

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.FolderDelete
import androidx.compose.material.icons.rounded.Memory
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.VideogameAsset
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.seedlessds.app.R

/** Everything the library header can do. Each action is reachable at every width. */
class LibraryHeaderActions(
    val onMultiplayer: () -> Unit,
    val onSearch: () -> Unit,
    val onProfile: () -> Unit,
    val onOpenRom: () -> Unit,
    val onAddFolder: () -> Unit,
    val onRemoveFolder: () -> Unit,
    val onBootFirmware: () -> Unit,
    val onSettings: () -> Unit,
)

/**
 * The actions at the end of the title row.
 *
 * Compact (a phone in portrait): Search, Settings and a More menu that holds the file and folder
 * actions and the DS firmware. Multiplayer, the profile and the view mode move to [LibraryActionStrip].
 * Regular (landscape, tablets): every action sits in this row.
 *
 * It emits the row's children, so call it inside the title [Row].
 */
@Composable
fun LibraryTitleActions(
    compact: Boolean,
    actions: LibraryHeaderActions,
    profile: String,
    viewMode: RomLibrary.ViewMode,
    onViewMode: (RomLibrary.ViewMode) -> Unit,
) {
    val ctx = LocalContext.current
    if (compact) {
        var more by remember { mutableStateOf(false) }
        SeedlessHeaderAction(Icons.Rounded.Search, ctx.getString(R.string.library_search), actions.onSearch)
        SeedlessHeaderAction(Icons.Rounded.Settings, ctx.getString(R.string.common_settings), actions.onSettings)
        SeedlessHeaderAction(Icons.Rounded.MoreVert, ctx.getString(R.string.common_more), { more = true })
        if (more) {
            LibraryMoreDialog(
                onDismiss = { more = false },
                items = listOf(
                    LibraryMoreItem(Icons.Rounded.VideogameAsset, ctx.getString(R.string.library_open_rom), actions.onOpenRom),
                    LibraryMoreItem(Icons.Rounded.CreateNewFolder, ctx.getString(R.string.library_add_folder), actions.onAddFolder),
                    LibraryMoreItem(Icons.Rounded.FolderDelete, ctx.getString(R.string.library_remove_folder), actions.onRemoveFolder),
                    LibraryMoreItem(Icons.Rounded.Memory, ctx.getString(R.string.firmware_boot), actions.onBootFirmware),
                ),
            )
        }
    } else {
        MultiplayerHeaderButton(onClick = actions.onMultiplayer)
        Spacer(Modifier.width(SeedlessSpace.xs))
        SeedlessHeaderAction(Icons.Rounded.Search, ctx.getString(R.string.library_search), actions.onSearch)
        ProfileChip(profile, onClick = actions.onProfile)
        Spacer(Modifier.width(SeedlessSpace.xs))
        ViewModeToggle(viewMode, onViewMode)
        Spacer(Modifier.width(SeedlessSpace.xs))
        SeedlessHeaderAction(Icons.Rounded.VideogameAsset, ctx.getString(R.string.library_open_rom), actions.onOpenRom)
        SeedlessHeaderAction(Icons.Rounded.CreateNewFolder, ctx.getString(R.string.library_add_folder), actions.onAddFolder)
        SeedlessHeaderAction(Icons.Rounded.FolderDelete, ctx.getString(R.string.library_remove_folder), actions.onRemoveFolder)
        SeedlessHeaderAction(Icons.Rounded.Memory, ctx.getString(R.string.firmware_boot), actions.onBootFirmware)
        SeedlessHeaderAction(Icons.Rounded.Settings, ctx.getString(R.string.common_settings), actions.onSettings)
    }
}

/**
 * Compact only: the second header row. Multiplayer takes the free width and the profile and the
 * view mode keep their natural size. All three are [SeedlessSpace.control] tall and the row uses
 * the same edge as the title row and the filter chips.
 */
@Composable
fun LibraryActionStrip(
    actions: LibraryHeaderActions,
    profile: String,
    viewMode: RomLibrary.ViewMode,
    onViewMode: (RomLibrary.ViewMode) -> Unit,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(SeedlessSpace.sm),
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = SeedlessSpace.edge, end = SeedlessSpace.edge, bottom = SeedlessSpace.sm),
    ) {
        MultiplayerHeaderButton(
            Modifier.weight(1f),
            showKey = LocalConfiguration.current.screenWidthDp >= 400,
            onClick = actions.onMultiplayer,
        )
        ProfileChip(profile, Modifier.widthIn(max = 132.dp), actions.onProfile)
        ViewModeToggle(viewMode, onViewMode)
    }
}

private class LibraryMoreItem(val icon: ImageVector, val label: String, val onClick: () -> Unit)

@Composable
private fun LibraryMoreDialog(onDismiss: () -> Unit, items: List<LibraryMoreItem>) {
    val colors = seedless
    val ctx = LocalContext.current
    SeedlessDialog(onDismiss = onDismiss, width = 360.dp) {
        Text(
            text = ctx.getString(R.string.common_more),
            color = colors.text,
            fontFamily = SpaceGrotesk,
            fontSize = 16.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 12.dp),
        )
        Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
            items.forEach { item ->
                SeedlessRow(
                    modifier = Modifier.padding(vertical = 2.dp),
                    onClick = {
                        onDismiss()
                        item.onClick()
                    },
                ) {
                    Icon(item.icon, null, tint = colors.text2, modifier = Modifier.size(20.dp))
                    Text(
                        text = item.label,
                        color = colors.text,
                        fontFamily = Manrope,
                        fontSize = 13.5.sp,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.padding(start = 12.dp),
                    )
                }
            }
        }
        Spacer(Modifier.padding(bottom = 14.dp))
    }
}
