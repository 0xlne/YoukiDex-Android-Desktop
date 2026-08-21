package com.youki.dex.fragments

import android.content.ClipData
import android.content.Context
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.text.format.Formatter
import android.view.*
import android.widget.*
import androidx.core.content.FileProvider
import androidx.core.view.isVisible
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.GridLayoutManager
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.button.MaterialButton
import com.google.android.material.chip.Chip
import com.google.android.material.chip.ChipGroup
import com.youki.dex.R
import java.io.File
import java.text.SimpleDateFormat
import java.util.*

/**
 * Internal file picker — used instead of ACTION_GET_CONTENT / ACTION_OPEN_DOCUMENT
 *
 * Usage:
 *   InternalFilePickerFragment.show(
 *       parentFragmentManager,
 *       mimeTypes = listOf("font/ttf","font/otf"),
 *       onPicked  = { uri -> /* use the uri */ }
 *   )
 */
class InternalFilePickerFragment : Fragment() {

    companion object {
        private const val ARG_MIME    = "mime_types"
        private const val ARG_TITLE   = "title"
        private const val ARG_MULTI   = "multi_select"

        fun show(
            fm: androidx.fragment.app.FragmentManager,
            containerId: Int,
            // FIX (hardcoded Arabic text): the default here used to be fixed Arabic
            // text. Since this is a companion object function with no Context, it
            // can't read strings.xml directly, so we made the default null, and
            // onViewCreated below uses the actually-translated
            // getString(R.string.select_a_file) whenever no explicit title is
            // passed — instead of fixed Arabic text regardless of language.
            title: String? = null,
            mimeTypes: List<String> = listOf("*/*"),
            multiSelect: Boolean = false,
            onPicked: (List<Uri>) -> Unit
        ) {
            val frag = InternalFilePickerFragment().apply {
                arguments = Bundle().apply {
                    putStringArrayList(ARG_MIME, ArrayList(mimeTypes))
                    putString(ARG_TITLE, title)
                    putBoolean(ARG_MULTI, multiSelect)
                }
                this.onPicked = onPicked
            }
            fm.beginTransaction()
                .replace(containerId, frag, "file_picker")
                .addToBackStack("file_picker")
                .commit()
        }
    }

    var onPicked: ((List<Uri>) -> Unit)? = null

    // ── Views ──────────────────────────────────────────────────────────────
    private lateinit var toolbar       : MaterialToolbar
    private lateinit var breadcrumb    : LinearLayout
    private lateinit var breadcrumbScroll: HorizontalScrollView
    private lateinit var storageChips  : ChipGroup
    private lateinit var recycler      : RecyclerView
    private lateinit var emptyView     : TextView
    private lateinit var confirmBtn    : MaterialButton

    // ── State ──────────────────────────────────────────────────────────────
    private var currentDir   : File = Environment.getExternalStorageDirectory()
    private var allowedMimes : List<String> = listOf("*/*")
    private var multiSelect  : Boolean = false
    private var selectedFiles: MutableList<File> = mutableListOf()
    private var isGridLayout : Boolean = false

    private val storageRoots: List<Pair<String, File>> by lazy {
        // FIX (hardcoded Arabic text): "Internal storage"/"SD card"/"Storage" used
        // to be fixed Arabic text. Now they're read from strings.xml
        // (internal_storage/sd_card/storage, which already existed and were
        // translated), so they follow the app's actual language.
        val list = mutableListOf<Pair<String, File>>()
        val internal = Environment.getExternalStorageDirectory()
        if (internal.canRead()) list.add(getString(R.string.internal_storage) to internal)
        requireContext().getExternalFilesDirs(null).drop(1).forEach { f ->
            val sd = f?.parentFile?.parentFile?.parentFile?.parentFile
            if (sd != null && sd.canRead() && sd != internal)
                list.add(getString(R.string.sd_card) to sd)
        }
        list.ifEmpty { listOf(getString(R.string.storage) to internal) }
    }

    // ─────────────────────────────────────────────────────────────────────
    override fun onCreate(s: Bundle?) {
        super.onCreate(s)
        allowedMimes = arguments?.getStringArrayList(ARG_MIME) ?: listOf("*/*")
        multiSelect  = arguments?.getBoolean(ARG_MULTI) ?: false
    }

    override fun onCreateView(i: LayoutInflater, c: ViewGroup?, s: Bundle?): View =
        i.inflate(R.layout.fragment_file_picker, c, false)

    override fun onViewCreated(v: View, s: Bundle?) {
        super.onViewCreated(v, s)

        toolbar        = v.findViewById(R.id.picker_toolbar)
        breadcrumb     = v.findViewById(R.id.picker_breadcrumb)
        breadcrumbScroll = v.findViewById(R.id.picker_breadcrumb_scroll)
        storageChips   = v.findViewById(R.id.picker_storage_chips)
        recycler       = v.findViewById(R.id.picker_recycler)
        emptyView      = v.findViewById(R.id.picker_empty)
        confirmBtn     = v.findViewById(R.id.picker_confirm_btn)

        // FIX (hardcoded Arabic text): the fallback here used to be fixed Arabic
        // text; now it uses the actually-translated getString(R.string.select_a_file).
        toolbar.title = arguments?.getString(ARG_TITLE) ?: getString(R.string.select_a_file)
        toolbar.setNavigationOnClickListener { parentFragmentManager.popBackStack() }

        confirmBtn.isVisible = multiSelect
        confirmBtn.setOnClickListener { confirmSelection() }

        setupStorageChips()
        navigate(currentDir)
    }

    // ── Storage chips ──────────────────────────────────────────────────────
    private fun setupStorageChips() {
        storageChips.removeAllViews()
        storageRoots.forEachIndexed { i, (label, file) ->
            val chip = Chip(requireContext()).apply {
                text = label; isCheckable = true; isChecked = i == 0
                setOnCheckedChangeListener { _, checked -> if (checked) navigate(file) }
            }
            storageChips.addView(chip)
        }
    }

    // ── Navigate ───────────────────────────────────────────────────────────
    private fun navigate(dir: File) {
        currentDir = dir
        updateBreadcrumb()
        loadFiles()
    }

    private fun navigateUp(): Boolean {
        val root = storageRoots.firstOrNull {
            currentDir.canonicalPath.startsWith(it.second.canonicalPath)
        }?.second
        val parent = currentDir.parentFile
        if (parent != null && parent.canRead() && currentDir != root) {
            navigate(parent); return true
        }
        return false
    }

    private fun updateBreadcrumb() {
        breadcrumb.removeAllViews()
        val root = storageRoots.firstOrNull {
            currentDir.canonicalPath.startsWith(it.second.canonicalPath)
        } ?: return

        val parts = mutableListOf<Pair<String, File>>()
        var f: File? = currentDir
        while (f != null && f.canonicalPath != root.second.canonicalPath) {
            parts.add(0, f.name to f); f = f.parentFile
        }
        parts.add(0, root.first to root.second)

        parts.forEachIndexed { i, (label, file) ->
            if (i > 0) breadcrumb.addView(TextView(requireContext()).apply {
                text = " › "; textSize = 12f
                setTextColor(0xFF888888.toInt())
            })
            breadcrumb.addView(TextView(requireContext()).apply {
                text = label; textSize = 13f
                val isLast = i == parts.lastIndex
                setTextColor(if (isLast) 0xFFFFFFFF.toInt() else 0xFF888888.toInt())
                val p = (6 * resources.displayMetrics.density).toInt()
                setPadding(p, p / 2, p, p / 2)
                if (!isLast) setOnClickListener { navigate(file) }
            })
        }
        breadcrumbScroll.post { breadcrumbScroll.fullScroll(View.FOCUS_RIGHT) }
    }

    // ── Load files ─────────────────────────────────────────────────────────
    private fun loadFiles() {
        val entries = currentDir.listFiles()?.toList() ?: emptyList()
        val dirs   = entries.filter { it.isDirectory }.sortedBy { it.name.lowercase() }
        val files  = entries.filter { it.isFile && matchesMime(it) }
            .sortedBy { it.name.lowercase() }
        val all = dirs + files

        emptyView.isVisible  = all.isEmpty()
        recycler.isVisible   = all.isNotEmpty()

        recycler.layoutManager = if (isGridLayout)
            GridLayoutManager(requireContext(), 3)
        else LinearLayoutManager(requireContext())

        recycler.adapter = PickerAdapter(all)
    }

    // ── MIME filter ────────────────────────────────────────────────────────
    private fun matchesMime(f: File): Boolean {
        if (allowedMimes.any { it == "*/*" }) return true
        val ext = f.extension.lowercase()
        val extMime = extToMime(ext)
        return allowedMimes.any { allowed ->
            when {
                allowed == extMime            -> true
                allowed.endsWith("/*")        -> extMime.startsWith(allowed.removeSuffix("/*"))
                allowed == "*/*"              -> true
                else                          -> false
            }
        }
    }

    private fun extToMime(ext: String) = when (ext) {
        "ttf","otf"             -> "font/$ext"
        "woff","woff2"          -> "font/$ext"
        "jpg","jpeg"            -> "image/jpeg"
        "png"                   -> "image/png"
        "gif"                   -> "image/gif"
        "webp"                  -> "image/webp"
        "mp4"                   -> "video/mp4"
        "mkv"                   -> "video/x-matroska"
        "webm"                  -> "video/webm"
        "mp3"                   -> "audio/mpeg"
        "pdf"                   -> "application/pdf"
        "zip"                   -> "application/zip"
        "apk"                   -> "application/vnd.android.package-archive"
        "json"                  -> "application/json"
        "xml"                   -> "application/xml"
        "txt"                   -> "text/plain"
        else                    -> "application/octet-stream"
    }

    // ── Confirm selection ──────────────────────────────────────────────────
    private fun confirmSelection() {
        if (selectedFiles.isEmpty()) return
        val uris = selectedFiles.map {
            FileProvider.getUriForFile(requireContext(),
                "${requireContext().packageName}.provider", it)
        }
        onPicked?.invoke(uris)
        parentFragmentManager.popBackStack()
    }

    private fun pickSingle(f: File) {
        val uri = FileProvider.getUriForFile(requireContext(),
            "${requireContext().packageName}.provider", f)
        onPicked?.invoke(listOf(uri))
        parentFragmentManager.popBackStack()
    }

    // ── Back press ────────────────────────────────────────────────────────
    fun onBackPressed() = navigateUp()

    // ── Adapter ────────────────────────────────────────────────────────────
    // FIX (performance): listFiles() for counting items used to be called from
    // disk again on every onBindViewHolder for every folder — slow with large
    // folders and repeated on every scroll. A simple in-memory cache (keyed by
    // folder path) is enough for the fragment's lifetime.
    private val dirItemCountCache = HashMap<String, Int>()

    inner class PickerAdapter(private val items: List<File>) :
        RecyclerView.Adapter<PickerAdapter.VH>() {

        override fun getItemViewType(p: Int) = if (isGridLayout) 1 else 0

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val icon      : ImageView = v.findViewById(R.id.fm_item_icon)
            val thumbnail : ImageView = v.findViewById(R.id.fm_item_thumbnail)
            val name      : TextView  = v.findViewById(R.id.fm_item_name)
            val size      : TextView  = v.findViewById(R.id.fm_item_size)
            val date      : TextView  = v.findViewById(R.id.fm_item_date)
            val menuBtn   : View      = v.findViewById(R.id.fm_item_menu)
        }

        override fun onCreateViewHolder(p: ViewGroup, t: Int): VH {
            val layout = if (t == 1) R.layout.item_fm_file_grid else R.layout.item_fm_file_list
            return VH(LayoutInflater.from(p.context).inflate(layout, p, false))
        }

        override fun getItemCount() = items.size

        override fun onBindViewHolder(h: VH, pos: Int) {
            val f = items[pos]
            h.name.text = f.name
            h.menuBtn.isVisible = false   // we don't need a menu in the picker
            h.date.text = SimpleDateFormat("yyyy/MM/dd", Locale.getDefault())
                .format(Date(f.lastModified()))

            val isSelected = f in selectedFiles

            if (f.isDirectory) {
                val count = dirItemCountCache.getOrPut(f.absolutePath) { f.listFiles()?.size ?: 0 }
                // FIX (hardcoded Arabic text): "$count items" used to be fixed Arabic
                // text; now it uses the same translated n_elements resource used
                // across the rest of the app (e.g. FileManagerFragment), so it
                // follows the current language and respects each language's own
                // plural rules where they exist.
                h.size.text  = getString(R.string.n_elements, count)
                h.icon.setImageResource(R.drawable.ic_folder)
                h.thumbnail.isVisible = false
                h.icon.isVisible = true
            } else {
                h.size.text = Formatter.formatShortFileSize(requireContext(), f.length())
                val ext = f.extension.lowercase()
                h.thumbnail.isVisible = false
                h.icon.isVisible = true
                iconFallback(h, ext) // instant default, gets swapped for the real image once ready
                if (com.youki.dex.utils.ThumbnailLoader.isPreviewable(ext)) {
                    com.youki.dex.utils.ThumbnailLoader.load(f, h.itemView) { bmp ->
                        h.thumbnail.setImageBitmap(bmp)
                        h.thumbnail.isVisible = true
                        h.icon.isVisible = false
                    }
                }
            }

            // Highlight the selected items
            (h.itemView as? com.google.android.material.card.MaterialCardView)
                ?.strokeWidth = if (isSelected) 3 else 0

            h.itemView.setOnClickListener {
                if (f.isDirectory) { navigate(f); return@setOnClickListener }
                if (multiSelect) {
                    if (isSelected) selectedFiles.remove(f) else selectedFiles.add(f)
                    notifyItemChanged(pos)
                    // FIX (hardcoded Arabic text): "Confirm ($count)" used to be fixed
                    // Arabic text; now it uses the translated confirm_n, so it follows the
                    // app's current language.
                    confirmBtn.text = getString(R.string.confirm_n, selectedFiles.size)
                } else pickSingle(f)
            }
        }

        private fun iconFallback(h: VH, ext: String) {
            h.thumbnail.isVisible = false
            h.icon.isVisible = true
            h.icon.setImageResource(when (ext) {
                "mp4","mkv","webm","mov","3gp" -> R.drawable.ic_video_file
                "mp3","flac","aac","ogg","wav" -> R.drawable.ic_audio_file
                "jpg","jpeg","png","gif","webp" -> R.drawable.ic_image
                "pdf"                           -> R.drawable.ic_pdf
                "zip","rar","7z","tar","gz"     -> R.drawable.ic_zip
                "ttf","otf","woff","woff2"      -> R.drawable.ic_font
                "apk"                           -> R.drawable.ic_apk
                "txt","log","md","json","xml"   -> R.drawable.ic_text_file
                else                            -> R.drawable.ic_file
            })
        }
    }
}
