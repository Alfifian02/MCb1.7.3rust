package com.seedlessds.app.emu

import com.seedlessds.app.R
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Code
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Memory
import androidx.compose.material.icons.rounded.Upload
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.documentfile.provider.DocumentFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

object BiosTools {
    private val REAL = mapOf(
        "nds_bios_arm7.bin" to 16384L, "nds_bios_arm9.bin" to 4096L, "nds_firmware.bin" to 262144L
    )
    private fun sysBios(ctx: Context) = File(EmuAssets.systemDir(ctx), "system").apply { mkdirs() }

    fun status(ctx: Context): Map<String, Boolean> =
        REAL.mapValues { (name, size) -> File(sysBios(ctx), name).let { it.exists() && it.length() == size } }

    fun install(ctx: Context, uris: List<Uri>): Int {
        var ok = 0
        for (uri in uris) {
            val len = runCatching { ctx.contentResolver.openAssetFileDescriptor(uri, "r")?.length }.getOrNull() ?: continue
            val name = REAL.entries.firstOrNull { it.value == len }?.key ?: continue
            runCatching {
                ctx.contentResolver.openInputStream(uri)?.use { input ->
                    File(sysBios(ctx), name).outputStream().use { input.copyTo(it) }
                }
                ok++
            }
        }
        return ok
    }
}

object ScriptTools {
    private const val MAX_BYTES = 1L shl 20

    fun dir(ctx: Context): File = File(EmuAssets.systemDir(ctx), "scripts").apply { mkdirs() }

    fun list(ctx: Context): List<String> =
        dir(ctx).listFiles { f -> f.isFile && f.name.endsWith(".lua") }
            ?.map { it.name }?.sortedBy { it.lowercase() } ?: emptyList()

    private fun displayName(ctx: Context, uri: Uri): String? =
        runCatching {
            ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment

    private fun readLimited(ctx: Context, uri: Uri): ByteArray? {
        val stream = ctx.contentResolver.openInputStream(uri) ?: return null
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(8192)
        var total = 0L
        stream.use {
            while (true) {
                val n = it.read(buf)
                if (n < 0) break
                total += n
                if (total > MAX_BYTES) return null
                out.write(buf, 0, n)
            }
        }
        return out.toByteArray()
    }

    /** Returns (imported, skipped). The extension is normalised to lowercase ".lua" because the core looks for exactly that. */
    fun install(ctx: Context, uris: List<Uri>): Pair<Int, Int> {
        var ok = 0
        var skipped = 0
        val target = dir(ctx)
        for (uri in uris) {
            val raw = displayName(ctx, uri)?.substringAfterLast('/')?.substringAfterLast('\\')?.trim() ?: ""
            if (raw.length <= 4 || raw.startsWith(".") || !raw.endsWith(".lua", ignoreCase = true)) {
                skipped++; continue
            }
            val dest = File(target, raw.dropLast(4) + ".lua")
            val bytes = runCatching { readLimited(ctx, uri) }.getOrNull()
            if (bytes == null) { skipped++; continue }
            val tmp = File(target, dest.name + ".tmp")
            val written = runCatching {
                tmp.writeBytes(bytes)
                if (!tmp.renameTo(dest)) { dest.delete(); check(tmp.renameTo(dest)) }
            }.isSuccess
            if (written) ok++ else { tmp.delete(); skipped++ }
        }
        return ok to skipped
    }

    fun remove(ctx: Context, name: String): Boolean =
        name in list(ctx) && File(dir(ctx), name).delete()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SystemToolsScreen(onBack: () -> Unit) {
    val uiContext = androidx.compose.ui.platform.LocalContext.current
    val ctx = LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    androidx.activity.compose.BackHandler { onBack() }
    var refresh by remember { mutableIntStateOf(0) }
    var status by remember { mutableStateOf<String?>(null) }
    val bios = remember(refresh) { BiosTools.status(ctx) }

    val biosPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (!uris.isNullOrEmpty()) {
            val n = BiosTools.install(ctx, uris); refresh++
            status = if (n > 0) uiContext.resources.getQuantityString(R.plurals.tools_installed_bios_file_s, n, n) else uiContext.getString(R.string.tools_no_valid_bios_recognized_check_the_size)
        }
    }
    val scripts = remember(refresh) { ScriptTools.list(ctx) }
    var deleting by remember { mutableStateOf<String?>(null) }
    deleting?.let { name ->
        SeedlessConfirmDialog(
            title = uiContext.getString(R.string.common_delete_2, name),
            message = uiContext.getString(R.string.tools_script_delete_message),
            confirmLabel = uiContext.getString(R.string.common_delete),
            onConfirm = {
                ScriptTools.remove(ctx, name); deleting = null; refresh++
                status = uiContext.getString(R.string.tools_script_deleted, name)
            },
            onDismiss = { deleting = null },
        )
    }
    val scriptPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (!uris.isNullOrEmpty()) {
            val (ok, skipped) = ScriptTools.install(ctx, uris); refresh++
            status = if (ok > 0) {
                val done = uiContext.resources.getQuantityString(R.plurals.tools_imported_scripts, ok, ok)
                if (skipped > 0) done + " " + uiContext.getString(R.string.tools_scripts_skipped, skipped) else done
            } else uiContext.getString(R.string.tools_no_valid_scripts_imported)
        }
    }
    val exportPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        if (tree != null) scope.launch {
            status = uiContext.getString(R.string.tools_exporting)
            val n = withContext(Dispatchers.IO) { DataMigration.export(ctx, tree) }
            status = uiContext.resources.getQuantityString(R.plurals.tools_exported_files, n, n)
        }
    }

    SeedlessScreenScaffold(
        title = uiContext.getString(R.string.common_system_tools),
        subtitle = uiContext.getString(R.string.tools_bios_and_backup),
        onBack = onBack,
        modifier = Modifier.fillMaxSize(),
    ) { pad ->
        RowCursorHost(onCancel = onBack) {}
        val scroll = rememberScrollState()
        Column(
            Modifier.fillMaxSize().padding(pad)
                .verticalScrollbar(scroll, MaterialTheme.colorScheme.primary.copy(alpha = 0.4f))
                .verticalScroll(scroll)
        ) {
            Group(uiContext.getString(R.string.tools_nintendo_bios_optional))
            Text(
                uiContext.getString(R.string.tools_bios_description),
                fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, lineHeight = 17.sp,
                modifier = Modifier.padding(start = 18.dp, end = 18.dp, top = 4.dp, bottom = 6.dp)
            )
            bios.forEach { (name, present) -> BiosStatusRow(name, present) }

            ActionCard(
                icon = Icons.Rounded.Memory,
                title = uiContext.getString(R.string.tools_install_bios),
                desc = uiContext.getString(R.string.tools_choose_the_3_bios_files_they_are_validated_by),
                onClick = { biosPicker.launch(arrayOf("*/*")) }
            )

            Group(uiContext.getString(R.string.tools_lua_scripts))
            Text(
                uiContext.getString(R.string.tools_lua_scripts_description),
                fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, lineHeight = 17.sp,
                modifier = Modifier.padding(start = 18.dp, end = 18.dp, top = 4.dp, bottom = 6.dp)
            )
            ActionCard(
                icon = Icons.Rounded.Code,
                title = uiContext.getString(R.string.tools_import_lua_script),
                desc = uiContext.getString(R.string.tools_choose_one_or_more_lua_files),
                onClick = { scriptPicker.launch(arrayOf("*/*")) }
            )
            if (scripts.isEmpty()) {
                Text(
                    uiContext.getString(R.string.tools_no_scripts_installed),
                    fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 18.dp, end = 18.dp, top = 4.dp, bottom = 4.dp)
                )
            } else scripts.forEach { name -> ScriptRow(name) { deleting = name } }

            Group(uiContext.getString(R.string.tools_data_backup))
            Text(
                uiContext.getString(R.string.tools_export_your_save_states_cheats_config_and),
                fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, lineHeight = 17.sp,
                modifier = Modifier.padding(start = 18.dp, end = 18.dp, top = 4.dp, bottom = 6.dp)
            )
            ActionCard(
                icon = Icons.Rounded.Upload,
                title = uiContext.getString(R.string.tools_export_data_to_a_folder),
                desc = uiContext.getString(R.string.tools_copies_the_emulator_s_data_tree_to_the_folder),
                onClick = { exportPicker.launch(null) }
            )

            status?.let {
                Group(uiContext.getString(R.string.common_status))
                Card(
                    Modifier.fillMaxWidth().padding(18.dp, 4.dp),
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                    )
                ) {
                    Row(
                        Modifier.fillMaxWidth().padding(14.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Icon(Icons.Rounded.Download, null, tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(20.dp))
                        Text(it, color = MaterialTheme.colorScheme.onSurface, fontSize = 14.sp,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(start = 12.dp))
                    }
                }
            }

            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable private fun Group(title: String) {
    Text(
        title.uppercase(), fontWeight = FontWeight.Bold, fontSize = 11.5.sp, letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.fillMaxWidth().padding(start = 18.dp, end = 16.dp, top = 20.dp, bottom = 2.dp)
    )
}

@Composable private fun BiosStatusRow(name: String, present: Boolean) {
    val uiContext = androidx.compose.ui.platform.LocalContext.current
    val accent = if (present) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error
    Row(
        Modifier.fillMaxWidth().padding(start = 18.dp, end = 18.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            Modifier.size(42.dp).clip(RoundedCornerShape(12.dp))
                .background(accent.copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                if (present) Icons.Rounded.CheckCircle else Icons.Rounded.ErrorOutline, null,
                tint = accent, modifier = Modifier.size(24.dp)
            )
        }
        Column(Modifier.weight(1f).padding(start = 14.dp)) {
            Text(name, fontSize = 15.5.sp)
            Text(
                if (present) uiContext.getString(R.string.tools_installed_correctly) else uiContext.getString(R.string.tools_missing_or_wrong_size),
                fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

@Composable private fun ScriptRow(name: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick)
            .padding(start = 18.dp, end = 18.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            Modifier.size(42.dp).clip(RoundedCornerShape(12.dp))
                .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center
        ) { Icon(Icons.Rounded.Code, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(24.dp)) }
        Text(name, fontSize = 15.5.sp, modifier = Modifier.weight(1f).padding(start = 14.dp))
        Icon(Icons.Rounded.Delete, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable private fun ActionCard(icon: ImageVector, title: String, desc: String, onClick: () -> Unit) {
    Card(
        Modifier.fillMaxWidth().padding(18.dp, 6.dp).clickable(onClick = onClick),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
        )
    ) {
        Row(Modifier.fillMaxWidth().padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(42.dp).clip(RoundedCornerShape(12.dp))
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.15f)),
                contentAlignment = Alignment.Center
            ) { Icon(icon, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(24.dp)) }
            Column(Modifier.weight(1f).padding(start = 14.dp)) {
                Text(title, fontSize = 15.5.sp, fontWeight = FontWeight.SemiBold)
                Text(desc, fontSize = 12.5.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

object DataMigration {
    private val WHITELIST = setOf("backup", "cheats", "config", "savestates", "profiles",
        "system", "users", "backgrounds", "virtual_controller", "scripts", "game_database.xml", "usrcheat.dat")

    fun export(ctx: Context, treeUri: Uri): Int {
        val root = DocumentFile.fromTreeUri(ctx, treeUri) ?: return 0
        val src = EmuAssets.systemDir(ctx)
        var count = 0
        val dest = root.findFile("SeedlessDS_backup") ?: root.createDirectory("SeedlessDS_backup") ?: return 0
        for (child in src.listFiles() ?: emptyArray()) {
            if (child.name in WHITELIST) count += copyInto(ctx, child, dest)
        }
        return count
    }

    private fun copyInto(ctx: Context, src: File, destDir: DocumentFile): Int {
        var n = 0
        if (src.isDirectory) {
            val sub = destDir.findFile(src.name) ?: destDir.createDirectory(src.name) ?: return 0
            for (c in src.listFiles() ?: emptyArray()) n += copyInto(ctx, c, sub)
        } else {
            val mime = "application/octet-stream"
            val out = destDir.findFile(src.name) ?: destDir.createFile(mime, src.name) ?: return 0
            runCatching {
                ctx.contentResolver.openOutputStream(out.uri)?.use { o -> src.inputStream().use { it.copyTo(o) } }
                n = 1
            }
        }
        return n
    }
}
