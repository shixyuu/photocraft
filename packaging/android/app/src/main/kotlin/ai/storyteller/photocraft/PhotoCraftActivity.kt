package ai.storyteller.photocraft

import android.app.AlertDialog
import android.content.ClipData
import android.os.Build
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import androidx.documentfile.provider.DocumentFile
import com.google.androidgamesdk.GameActivity
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream

/**
 * GameActivity shell for the native editor.
 *
 * Rust writes `files/bridge/request.json`. This activity shows the system file, message, or
 * clipboard UI and writes `files/bridge/response.json`. Opened documents are copied to real
 * files because the editor reads paths, not content URIs.
 */
class PhotoCraftActivity : GameActivity() {
    private val handler = Handler(Looper.getMainLooper())
    private var pending: JSONObject? = null
    private val poll = object : Runnable {
        override fun run() {
            pollRequest()
            handler.postDelayed(this, 80)
        }
    }

    private val activityResult = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val req = pending ?: return@registerForActivityResult
        val data = result.data
        if (result.resultCode != RESULT_OK || data == null) {
            finishJson(req, JSONObject().put("cancel", true))
            return@registerForActivityResult
        }
        when (req.optString("op")) {
            "save" -> onSave(req, data)
            "folder" -> onFolder(req, data)
            else -> onOpen(req, data)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        handleIntent(intent)
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        handler.post(poll)
    }

    override fun onDestroy() {
        handler.removeCallbacks(poll)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    private fun bridgeDir(): File = File(filesDir, "bridge").apply { mkdirs() }

    private fun pollRequest() {
        if (pending != null) return
        val request = File(bridgeDir(), "request.json")
        if (!request.isFile) return
        val req = try {
            JSONObject(request.readText())
        } catch (_: Exception) {
            request.delete()
            return
        }
        request.delete()
        pending = req
        try {
            dispatch(req)
        } catch (error: Exception) {
            finishJson(req, JSONObject().put("error", error.message ?: "bridge failed"))
        }
    }

    private fun dispatch(req: JSONObject) {
        when (req.optString("op")) {
            "open" -> launchOpen(req)
            "save" -> launchSave(req)
            "folder" -> launchFolder()
            "message" -> showMessage(req)
            "open-url" -> {
                val url = req.optString("text")
                startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                finishJson(req, JSONObject())
            }
            "clipboard-set-text" -> {
                clipboard().setPrimaryClip(ClipData.newPlainText("PhotoCraft", req.optString("text")))
                finishJson(req, JSONObject())
            }
            "clipboard-get-text" -> {
                val text = clipboard().primaryClip?.getItemAt(0)?.coerceToText(this)?.toString().orEmpty()
                finishJson(req, JSONObject().put("text", text))
            }
            "clipboard-set-image" -> {
                val file = File(req.optString("path"))
                val uri = FileProvider.getUriForFile(this, "$packageName.files", file)
                val clip = ClipData.newUri(contentResolver, "PhotoCraft", uri)
                clipboard().setPrimaryClip(clip)
                finishJson(req, JSONObject())
            }
            "clipboard-get-image" -> finishJson(req, clipboardImage())
            else -> finishJson(req, JSONObject().put("error", "unknown request"))
        }
    }

    private fun launchOpen(req: JSONObject) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        val filters = req.optJSONArray("filters")
        if (filters != null && filters.length() > 0) {
            val mimes = Array(filters.length()) { index -> mimeFor(filters.optString(index)) }
            intent.putExtra(Intent.EXTRA_MIME_TYPES, mimes)
        }
        if (req.optBoolean("multiple")) intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        activityResult.launch(intent)
    }

    private fun launchSave(req: JSONObject) {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
            .setType(req.optString("mime", "application/octet-stream"))
            .putExtra(Intent.EXTRA_TITLE, req.optString("name", "image.png"))
        activityResult.launch(intent)
    }

    private fun launchFolder() {
        activityResult.launch(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE))
    }

    private fun onOpen(req: JSONObject, data: Intent) {
        val dir = File(filesDir, "imports").apply { mkdirs() }
        val paths = org.json.JSONArray()
        for (uri in urisOf(data)) {
            val dest = File(dir, uniqueName(displayName(uri)))
            contentResolver.openInputStream(uri)?.use { input ->
                FileOutputStream(dest).use { output -> input.copyTo(output) }
            }
            if (dest.isFile) paths.put(dest.absolutePath)
        }
        finishJson(req, if (paths.length() == 0) JSONObject().put("cancel", true) else JSONObject().put("paths", paths))
    }

    private fun onSave(req: JSONObject, data: Intent) {
        val uri = data.data ?: return finishJson(req, JSONObject().put("cancel", true))
        val dir = File(filesDir, "exports/${req.optLong("id")}").apply { mkdirs() }
        val dest = File(dir, safeName(req.optString("name", "image.png")))
        finishJson(req, JSONObject().put("paths", org.json.JSONArray().put(dest.absolutePath)))
        watchExport(dest, uri, 0, -1L, 0)
    }

    private fun watchExport(dest: File, uri: Uri, ticks: Int, last: Long, stable: Int) {
        if (ticks > 600) return
        val len = if (dest.isFile) dest.length() else -1L
        val nextStable = if (len > 0 && len == last) stable + 1 else 0
        if (nextStable >= 3) {
            contentResolver.openOutputStream(uri)?.use { output ->
                dest.inputStream().use { input -> input.copyTo(output) }
            }
            return
        }
        handler.postDelayed({ watchExport(dest, uri, ticks + 1, len, nextStable) }, 200)
    }

    private fun onFolder(req: JSONObject, data: Intent) {
        val tree = data.data ?: return finishJson(req, JSONObject().put("cancel", true))
        val root = DocumentFile.fromTreeUri(this, tree) ?: return finishJson(req, JSONObject().put("cancel", true))
        val dir = File(filesDir, "imports/tree-${req.optLong("id")}").apply { mkdirs() }
        for (child in root.listFiles()) {
            if (!child.isFile) continue
            val dest = File(dir, safeName(child.name ?: "file"))
            contentResolver.openInputStream(child.uri)?.use { input ->
                FileOutputStream(dest).use { output -> input.copyTo(output) }
            }
        }
        finishJson(req, JSONObject().put("paths", org.json.JSONArray().put(dir.absolutePath)))
    }

    private fun showMessage(req: JSONObject) {
        val buttons = req.optString("buttons", "ok")
        val dialog = AlertDialog.Builder(this)
            .setTitle(req.optString("title", "PhotoCraft"))
            .setMessage(req.optString("text"))
            .setCancelable(true)
            .setOnCancelListener { finishJson(req, JSONObject().put("text", "cancel").put("cancel", true)) }
        when (buttons) {
            "yes-no", "yes-no-cancel" -> {
                dialog.setPositiveButton("Yes") { _, _ -> finishJson(req, JSONObject().put("text", "yes")) }
                dialog.setNegativeButton("No") { _, _ -> finishJson(req, JSONObject().put("text", "no")) }
                if (buttons == "yes-no-cancel") {
                    dialog.setNeutralButton("Cancel") { _, _ -> finishJson(req, JSONObject().put("text", "cancel")) }
                }
            }
            "ok-cancel" -> {
                dialog.setPositiveButton("OK") { _, _ -> finishJson(req, JSONObject().put("text", "ok")) }
                dialog.setNegativeButton("Cancel") { _, _ -> finishJson(req, JSONObject().put("text", "cancel")) }
            }
            else -> dialog.setPositiveButton("OK") { _, _ -> finishJson(req, JSONObject().put("text", "ok")) }
        }
        dialog.show()
    }

    private fun clipboardImage(): JSONObject {
        val clip = clipboard().primaryClip ?: return JSONObject().put("error", "clipboard empty")
        val uri = clip.getItemAt(0).uri ?: return JSONObject()
        val dest = File(File(filesDir, "imports").apply { mkdirs() }, uniqueName(displayName(uri)))
        contentResolver.openInputStream(uri)?.use { input ->
            FileOutputStream(dest).use { output -> input.copyTo(output) }
        }
        return if (dest.isFile) JSONObject().put("path", dest.absolutePath) else JSONObject().put("error", "clipboard image unreadable")
    }

    private fun clipboard(): ClipboardManager = getSystemService(ClipboardManager::class.java)

    private fun handleIntent(intent: Intent?) {
        if (intent == null) return
        val uri = when (intent.action) {
            Intent.ACTION_VIEW -> intent.data
            Intent.ACTION_SEND -> if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            else -> null
        } ?: return
        val dir = File(filesDir, "imports").apply { mkdirs() }
        val dest = File(dir, uniqueName(displayName(uri)))
        contentResolver.openInputStream(uri)?.use { input ->
            FileOutputStream(dest).use { output -> input.copyTo(output) }
        }
        if (dest.isFile) {
            File(filesDir, "inbox").mkdirs()
            File(filesDir, "inbox/launch.txt").writeText(dest.absolutePath)
        }
    }

    private fun finishJson(req: JSONObject, body: JSONObject) {
        body.put("id", req.optLong("id"))
        val dir = bridgeDir()
        val tmp = File(dir, "response.json.tmp")
        tmp.writeText(body.toString())
        tmp.renameTo(File(dir, "response.json"))
        if (pending === req) pending = null
    }

    private fun urisOf(data: Intent): List<Uri> {
        val clip = data.clipData
        if (clip != null && clip.itemCount > 0) {
            return (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
        }
        return listOfNotNull(data.data)
    }

    private fun displayName(uri: Uri): String {
        var name = "image"
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) name = cursor.getString(0) ?: name
        }
        return safeName(name)
    }

    private fun uniqueName(name: String): String = "${System.currentTimeMillis()}-$name"

    private fun safeName(raw: String): String {
        val cleaned = raw.replace(Regex("""[^A-Za-z0-9._-]"""), "_").trim('_')
        return cleaned.ifEmpty { "image" }.take(80)
    }

    private fun mimeFor(ext: String): String = when (ext.lowercase()) {
        "png" -> "image/png"
        "jpg", "jpeg" -> "image/jpeg"
        "gif" -> "image/gif"
        "webp" -> "image/webp"
        "psd", "psb" -> "image/vnd.adobe.photoshop"
        "tif", "tiff" -> "image/tiff"
        else -> "application/octet-stream"
    }
}
