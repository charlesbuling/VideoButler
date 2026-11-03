// Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

package dev.videobutler.video_butler

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.DocumentsContract
import android.system.Os
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.effect.Presentation
import androidx.media3.transformer.Composition
import androidx.media3.transformer.EditedMediaItem
import androidx.media3.transformer.Effects
import androidx.media3.transformer.ExportException
import androidx.media3.transformer.ExportResult
import androidx.media3.transformer.Transformer
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Asks for read access to the user's videos and music on first launch, and brings the
 * models in: the small ones bundled in the APK, the large ones from the OBB
 * folder the installer filled, or from a model pack the user copied onto the
 * device.
 *
 * The app browses shared storage through the engine rather than through a
 * system picker, so that one picker works on every platform (the web build
 * cannot see the filesystem at all). On Android that means the app itself
 * needs permission to read media; without it the picker lists nothing.
 *
 * Model files are not media, so the permission does not reach them: a model
 * pack is chosen with the system's folder picker instead, which grants this
 * app that one folder, and copied from there into the app's library.
 *
 * Only the platform APIs are used here, so this adds no dependency.
 */
class MainActivity : FlutterActivity() {
    private var channel: MethodChannel? = null
    private val thermalLooper = Handler(Looper.getMainLooper())
    private var lastThermal = ""
    private var editorsChannel: MethodChannel? = null
    private var pendingImport: PendingImport? = null

    /** A folder import waiting for the user to pick the folder. */
    private class PendingImport(
        val files: List<Pair<String, Long>>,
        val prefer: List<List<Pair<String, Long>>>,
        /** Files of models this device cannot run: never copied, only reported. */
        val skip: List<String>,
        val result: MethodChannel.Result,
    )

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Where a run may write. The picker browses shared storage, where the
        // app can read a video but cannot create a project folder beside it,
        // so the engine is told a directory this app owns. Set as a real
        // process variable, which is what the Rust engine reads.
        Os.setenv("VB_WORKDIR", filesDir.absolutePath, true)
        // Where the editor's videos go: Movies/VideoButler, which the user
        // can reach and the hand-off already writes to. App-private storage
        // would keep them out of sight.
        Os.setenv(
            "VB_EXPORT_DIR",
            File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MOVIES), "VideoButler").absolutePath,
            true,
        )
        // Models that came with the installer. An APK holds 4 GB at most,
        // so its install script puts the rest in this app's OBB folder, and
        // the engine reads them there as it reads a Mac bundle's: in place,
        // never deleted from inside the app. Made here, so that the folder
        // is the app's own before the script copies anything into it.
        obbDir?.let { obb ->
            val bundled = File(obb, "models").also { it.mkdirs() }
            Os.setenv("VB_BUNDLED_MODELS", bundled.absolutePath, true)
        }
        noteInstalledApps()
        noteThermal()

        // Footage and music. From Android 13 videos, photos and music are
        // three permissions: an app granted only video sees no photo and no
        // song at all. One installed before a kind was asked for is asked
        // again for what it lacks.
        val wanted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            arrayOf(
                Manifest.permission.READ_MEDIA_VIDEO,
                Manifest.permission.READ_MEDIA_IMAGES,
                Manifest.permission.READ_MEDIA_AUDIO,
            )
        } else {
            arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        } + if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            // Where each video was filmed, which Android removes from what
            // an app reads without this: the video butler's trips and map.
            arrayOf(Manifest.permission.ACCESS_MEDIA_LOCATION)
        } else {
            emptyArray()
        }
        val missing = wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), REQUEST_READ_MEDIA)
        }
    }

    override fun onDestroy() {
        thermalLooper.removeCallbacksAndMessages(null)
        super.onDestroy()
    }

    /**
     * How hot the phone is, for the engine's heat guard (vb-pipeline's
     * thermal.rs), which cannot ask the power service itself: the thermal
     * status and the headroom forecast ten seconds ahead, written to
     * `thermal` beside the library every two seconds when they change.
     * Android allows the headroom to be asked about once a second.
     */
    private fun noteThermal() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val power = getSystemService(PowerManager::class.java) ?: return
        val status = power.currentThermalStatus
        val headroom = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            power.getThermalHeadroom(10)
        } else {
            Float.NaN
        }
        val text = "status=$status\nheadroom=${"%.2f".format(java.util.Locale.ROOT, headroom)}\n" +
            thermalThresholds(power)
        if (text != lastThermal) {
            try {
                val part = File(filesDir, "thermal.part")
                part.writeText(text)
                part.renameTo(File(filesDir, "thermal"))
                lastThermal = text
            } catch (e: Exception) {
                // No reading, and the guard does nothing: the same as a
                // phone that reports none.
            }
        }
        thermalLooper.postDelayed({ noteThermal() }, 2000)
    }

    /**
     * The device's own headroom at "moderate" and "severe", which the guard
     * slows and pauses at, where Android 15 reports them. The S21 says 0.93
     * and 1.0; elsewhere the engine assumes the same.
     */
    private fun thermalThresholds(power: PowerManager): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) return ""
        val own = try {
            power.thermalHeadroomThresholds
        } catch (e: Exception) {
            return ""
        }
        val warm = own[PowerManager.THERMAL_STATUS_MODERATE]
        val hot = own[PowerManager.THERMAL_STATUS_SEVERE]
        return buildString {
            if (warm != null && !warm.isNaN()) append("warm=$warm\n")
            if (hot != null && !hot.isNaN()) append("hot=$hot\n")
        }
    }

    override fun onResume() {
        super.onResume()
        // An editor installed while this app was in the background is seen
        // on return.
        noteInstalledApps()
    }

    /**
     * Which video editors are installed, for the engine, which cannot ask the
     * package manager itself: one `package=version` a line in
     * `installed-apps` beside the library (vb-export's INSTALLED_APPS).
     * Written whole and renamed into place, so the engine never reads half.
     */
    private fun noteInstalledApps() {
        val lines = EDITOR_PACKAGES.mapNotNull { pkg ->
            try {
                val info = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    packageManager.getPackageInfo(pkg, PackageManager.PackageInfoFlags.of(0))
                } else {
                    @Suppress("DEPRECATION")
                    packageManager.getPackageInfo(pkg, 0)
                }
                "$pkg=${info.versionName ?: ""}"
            } catch (e: PackageManager.NameNotFoundException) {
                null
            }
        }
        try {
            val part = File(filesDir, "$INSTALLED_APPS.part")
            part.writeText(lines.joinToString("") { "$it\n" })
            part.renameTo(File(filesDir, INSTALLED_APPS))
        } catch (e: Exception) {
            // The editors then read as not installed, which is what the
            // self-test says; nothing else depends on it.
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val models = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
        models.setMethodCallHandler { call, result -> handle(call, result) }
        channel = models
        editorsChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, EDITORS_CHANNEL)
            .also { it.setMethodCallHandler { call, result -> handleEditors(call, result) } }
    }

    /**
     * A cut, to a phone editor. A phone editor takes no draft from another
     * app, but 剪映 opens videos shared with it as a new project, on its
     * timeline in the order given -- checked on a real phone. It takes each
     * video whole, so every clip is first cut to its span (`handOff`), into
     * Movies/VideoButler, where the editor can read it and the user can see
     * it; `openInEditor` shares files that are already cut.
     */
    private fun handleEditors(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "openInEditor" -> {
                val pkg = call.argument<String>("package") ?: return result.error("args", "no package", null)
                val paths = call.argument<List<String>>("paths") ?: emptyList()
                if (paths.isEmpty()) return result.error("args", "no videos", null)
                Thread { shareToEditor(pkg, paths, result) }.start()
            }
            "handOff" -> handOff(call, result)
            else -> result.notImplemented()
        }
    }

    /**
     * Cut each clip to its span, one after another, then share them in order.
     * Transformer re-encodes on the phone's own codecs, so the cut is
     * frame-exact rather than snapped to the nearest keyframe, and a portrait
     * clip stays upright. A photo becomes a clip that holds the picture for
     * its span, since the editor takes videos.
     */
    private fun handOff(call: MethodCall, result: MethodChannel.Result) {
        val pkg = call.argument<String>("package") ?: return result.error("args", "no package", null)
        val name = call.argument<String>("folder") ?: "Video Butler"
        val clips = call.argument<List<Map<String, Any?>>>("clips") ?: emptyList()
        if (clips.isEmpty()) return result.error("args", "no clips", null)
        val frame = call.argument<Map<String, Any?>>("frame")
        val frameWidth = (frame?.get("width") as? Number)?.toInt()
        val frameHeight = (frame?.get("height") as? Number)?.toInt()

        // A folder of its own each time: never write over what an editor's
        // project may still be using.
        val base = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MOVIES), "VideoButler")
        val safe = name.replace(Regex("[\\\\/:*?\"<>|]"), "_").trim().ifEmpty { "Video Butler" }
        var dir = File(base, safe)
        var n = 2
        while (dir.exists()) {
            dir = File(base, "$safe-$n")
            n++
        }
        if (!dir.mkdirs()) return result.error("folder", "cannot create ${dir.absolutePath}", null)

        val outputs = ArrayList<String>()
        fun next(i: Int) {
            if (i == clips.size) {
                Thread { shareToEditor(pkg, outputs, result, dir.absolutePath) }.start()
                return
            }
            val clip = clips[i]
            val source = File(clip["path"] as String)
            val inUs = (clip["inUs"] as Number).toLong()
            val outUs = (clip["outUs"] as Number).toLong()
            val still = clip["still"] == true
            val item = if (still) {
                MediaItem.Builder()
                    .setUri(Uri.fromFile(source))
                    .setImageDurationMs((outUs - inUs) / 1000)
                    .build()
            } else {
                MediaItem.Builder()
                    .setUri(Uri.fromFile(source))
                    .setClippingConfiguration(
                        MediaItem.ClippingConfiguration.Builder()
                            .setStartPositionUs(inUs)
                            .setEndPositionUs(outUs)
                            .build(),
                    )
                    .build()
            }
            val out = File(dir, "%02d.mp4".format(i + 1)).absolutePath
            // The editor takes its frame from the first clip: a first clip
            // the other way up from the cut is fitted into the cut's frame,
            // with the bars it would have there anyway, so the project comes
            // out the right way up. The rest go as they are.
            val width = (clip["width"] as? Number)?.toInt()
            val height = (clip["height"] as? Number)?.toInt()
            val edited = EditedMediaItem.Builder(item)
            if (still) edited.setFrameRate(STILL_FRAME_RATE)
            // A photo is fitted into the cut's frame too, first or not: a
            // phone's photo is twelve megapixels, more than its video
            // encoder takes.
            if (frameWidth != null && frameHeight != null && width != null && height != null &&
                (still || (i == 0 && (height > width) != (frameHeight > frameWidth)))
            ) {
                val scale = minOf(1.0, 1920.0 / maxOf(frameWidth, frameHeight))
                val w = ((frameWidth * scale).toInt() / 2) * 2
                val h = ((frameHeight * scale).toInt() / 2) * 2
                edited.setEffects(
                    Effects(
                        listOf(),
                        listOf(Presentation.createForWidthAndHeight(w, h, Presentation.LAYOUT_SCALE_TO_FIT)),
                    ),
                )
            }
            val transformer = Transformer.Builder(this)
            // A photo has no codec of its own to follow, and Transformer
            // then picks HEVC, whose software encoder may take nothing above
            // 480 x 270 (an emulator's does not); H.264, as the phone's
            // videos are, goes to the frame's size.
            if (still) transformer.setVideoMimeType(MimeTypes.VIDEO_H264)
            transformer
                .addListener(object : Transformer.Listener {
                    override fun onCompleted(composition: Composition, exportResult: ExportResult) {
                        outputs.add(out)
                        editorsChannel?.invokeMethod(
                            "handOffProgress",
                            mapOf("done" to i + 1, "total" to clips.size),
                        )
                        next(i + 1)
                    }

                    override fun onError(
                        composition: Composition,
                        exportResult: ExportResult,
                        exportException: ExportException,
                    ) {
                        result.error("trim", "${source.name}: ${exportException.message}", null)
                    }
                })
                .build()
                .start(edited.build(), out)
        }
        next(0)
    }

    /**
     * Share videos, in order, with one app. They are indexed in the media
     * store first, since the editor reads them through it.
     */
    private fun shareToEditor(
        pkg: String,
        paths: List<String>,
        result: MethodChannel.Result,
        folder: String? = null,
    ) {
        val uris = arrayOfNulls<Uri>(paths.size)
        val scanned = CountDownLatch(paths.size)
        MediaScannerConnection.scanFile(this, paths.toTypedArray(), null) { path, uri ->
            val i = paths.indexOf(path)
            if (i >= 0) uris[i] = uri
            scanned.countDown()
        }
        scanned.await(30, TimeUnit.SECONDS)
        val missing = paths.filterIndexed { i, _ -> uris[i] == null }
        if (missing.isNotEmpty()) {
            runOnUiThread { result.error("scan", "not in the media store: $missing", null) }
            return
        }
        val list = ArrayList(uris.map { it!! })
        val send = Intent(if (list.size == 1) Intent.ACTION_SEND else Intent.ACTION_SEND_MULTIPLE).apply {
            type = "video/*"
            setPackage(pkg)
            if (list.size == 1) {
                putExtra(Intent.EXTRA_STREAM, list[0])
            } else {
                putParcelableArrayListExtra(Intent.EXTRA_STREAM, list)
            }
            // The grant covers every item only when they are in the clip
            // data as well.
            clipData = ClipData.newRawUri(null, list[0]).also { clip ->
                list.drop(1).forEach { clip.addItem(ClipData.Item(it)) }
            }
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        runOnUiThread {
            try {
                startActivity(send)
                result.success(mapOf("shared" to list.size, "folder" to folder))
            } catch (e: Exception) {
                result.error("open", e.message, null)
            }
        }
    }

    /** The engine's library of models: where the Rust side looks for them. */
    private fun modelsDir(): File = File(filesDir, "library/models").also { it.mkdirs() }

    private fun handle(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "prepareBundled" -> Thread {
                val copied = try {
                    copyBundled()
                } catch (e: Exception) {
                    runOnUiThread { result.error("bundled", e.message, null) }
                    return@Thread
                }
                runOnUiThread { result.success(copied) }
            }.start()
            "importFolder" -> {
                fun parse(list: List<Map<String, Any>>?) = (list ?: emptyList()).map {
                    (it["name"] as String) to ((it["bytes"] as? Number)?.toLong() ?: -1L)
                }
                val files = parse(call.argument<List<Map<String, Any>>>("files"))
                // Alternatives, best first: of these only the first whose
                // files are all in the folder is imported -- the largest
                // caption model the phone can run, not every size in a pack.
                val prefer = (call.argument<List<List<Map<String, Any>>>>("prefer") ?: emptyList())
                    .map { parse(it) }
                val skip = call.argument<List<String>>("skip") ?: emptyList()
                pendingImport = PendingImport(files, prefer, skip, result)
                val pick = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                @Suppress("DEPRECATION")
                startActivityForResult(pick, REQUEST_MODEL_FOLDER)
            }
            else -> result.notImplemented()
        }
    }

    /**
     * Copy the models bundled in the APK into the library, once. A file
     * already there at the same size is left alone, so later launches cost a
     * directory listing. Returns how many were copied.
     */
    private fun copyBundled(): Int {
        val names = assets.list("models") ?: return 0
        var copied = 0
        for (name in names) {
            val dest = File(modelsDir(), name)
            val size = try {
                assets.openFd("models/$name").use { it.length }
            } catch (e: Exception) {
                -1L
            }
            if (dest.isFile && (size < 0 || dest.length() == size)) continue
            val part = File(modelsDir(), "$name.part")
            assets.open("models/$name").use { input ->
                FileOutputStream(part).use { output -> input.copyTo(output, BUFFER) }
            }
            if (!part.renameTo(dest)) throw IllegalStateException("could not install $name")
            copied++
        }
        return copied
    }

    @Deprecated("The platform's own API; nothing here needs the AndroidX result API.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_MODEL_FOLDER) return
        val pending = pendingImport ?: return
        pendingImport = null
        val tree = data?.data
        if (resultCode != Activity.RESULT_OK || tree == null) {
            pending.result.success(mapOf("cancelled" to true))
            return
        }
        Thread { importFrom(tree, pending) }.start()
    }

    /** Every document in the chosen folder and the folders directly inside it. */
    private fun listTree(tree: Uri): Map<String, Pair<Uri, Long>> {
        val found = HashMap<String, Pair<Uri, Long>>()
        fun walk(parentId: String, depth: Int) {
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
            val columns = arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
            )
            contentResolver.query(children, columns, null, null, null)?.use { cursor ->
                while (cursor.moveToNext()) {
                    val id = cursor.getString(0)
                    val name = cursor.getString(1)
                    val mime = cursor.getString(2)
                    val size = if (cursor.isNull(3)) -1L else cursor.getLong(3)
                    if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                        if (depth == 0) walk(id, 1)
                    } else {
                        found[name] = DocumentsContract.buildDocumentUriUsingTree(tree, id) to size
                    }
                }
            }
        }
        walk(DocumentsContract.getTreeDocumentId(tree), 0)
        return found
    }

    /**
     * Copy each file the app asked for out of the chosen folder, under a
     * temporary name until it is whole and the size the catalogue expects.
     * Progress goes back to Dart as it happens.
     */
    private fun importFrom(tree: Uri, pending: PendingImport) {
        val always = pending.files
        val prefer = pending.prefer
        val result = pending.result
        val imported = ArrayList<String>()
        val already = ArrayList<String>()
        val failed = ArrayList<String>()
        try {
            val found = listTree(tree)
            val chosen = prefer.firstOrNull { group -> group.all { found.containsKey(it.first) } }
            val wanted = always + (chosen ?: emptyList())
            val todo = wanted.filter { found.containsKey(it.first) }
            val total = todo.sumOf { maxOf(it.second, found[it.first]!!.second, 0L) }.coerceAtLeast(1L)
            var done = 0L
            for ((name, bytes) in todo) {
                val (uri, size) = found[name]!!
                val expected = if (bytes > 0) bytes else size
                val dest = File(modelsDir(), name)
                if (dest.isFile && dest.length() == expected) {
                    already.add(name)
                    done += expected
                    continue
                }
                val part = File(modelsDir(), "$name.part")
                var copied = 0L
                var lastReport = 0L
                contentResolver.openInputStream(uri)?.use { input ->
                    FileOutputStream(part).use { output ->
                        val buffer = ByteArray(BUFFER)
                        while (true) {
                            val n = input.read(buffer)
                            if (n < 0) break
                            output.write(buffer, 0, n)
                            copied += n
                            if (copied - lastReport > REPORT_EVERY) {
                                lastReport = copied
                                val progress = done + copied
                                runOnUiThread {
                                    channel?.invokeMethod(
                                        "importProgress",
                                        mapOf("name" to name, "done" to progress, "total" to total),
                                    )
                                }
                            }
                        }
                    }
                }
                if ((expected > 0 && copied != expected) || !part.renameTo(dest)) {
                    part.delete()
                    failed.add(name)
                } else {
                    imported.add(name)
                }
                done += expected
            }
            val missing = wanted.map { it.first }.filter { !found.containsKey(it) }
            // In the folder, and left there: a model this device cannot run
            // is said to be too large rather than not found.
            val tooBig = pending.skip.filter { found.containsKey(it) }
            runOnUiThread {
                result.success(
                    mapOf(
                        "imported" to imported,
                        "alreadyInstalled" to already,
                        "failed" to failed,
                        "missing" to missing,
                        "tooBig" to tooBig,
                    ),
                )
            }
        } catch (e: Exception) {
            runOnUiThread { result.error("import", e.message, null) }
        }
    }

    private companion object {
        const val REQUEST_READ_MEDIA = 1

        /** The rate a photo's clip is written at: steady, and what a phone's cut runs at. */
        const val STILL_FRAME_RATE = 30
        const val REQUEST_MODEL_FOLDER = 2
        const val CHANNEL = "videobutler/models"
        const val EDITORS_CHANNEL = "videobutler/editors"
        const val BUFFER = 4 * 1024 * 1024
        const val REPORT_EVERY = 16L * 1024 * 1024
        const val INSTALLED_APPS = "installed-apps"

        // 剪映 and CapCut, as their app store listings name them.
        val EDITOR_PACKAGES = listOf("com.lemon.lv", "com.lemon.lvoverseas")
    }
}
