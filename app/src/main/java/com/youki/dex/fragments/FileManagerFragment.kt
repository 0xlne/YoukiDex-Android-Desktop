package com.youki.dex.fragments

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.text.format.Formatter
import android.view.*
import android.widget.*
import androidx.appcompat.app.AlertDialog
import androidx.core.content.FileProvider
import androidx.core.view.isVisible
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.GridLayoutManager
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import androidx.swiperefreshlayout.widget.SwipeRefreshLayout
import com.google.android.material.button.MaterialButton
import com.google.android.material.chip.Chip
import com.google.android.material.chip.ChipGroup
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.search.SearchBar
import com.google.android.material.snackbar.Snackbar
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout
import com.youki.dex.R
import java.io.File
import java.text.SimpleDateFormat
import java.util.*
import java.util.zip.ZipEntry
import java.util.zip.ZipFile
import java.util.zip.ZipOutputStream
import android.os.Build
import android.provider.Settings
import com.youki.dex.utils.VideoUtils

class FileManagerFragment : Fragment() {

    // ── Views ──────────────────────────────────────────────────────────────
    private lateinit var searchBar       : SearchBar
    private lateinit var breadcrumb      : LinearLayout
    private lateinit var breadcrumbScroll: HorizontalScrollView
    private lateinit var storageChips    : ChipGroup
    private lateinit var sortBtn         : MaterialButton
    private lateinit var viewToggle      : MaterialButton
    private lateinit var itemCount       : TextView
    private lateinit var recycler        : RecyclerView
    private lateinit var swipeRefresh    : SwipeRefreshLayout
    private lateinit var emptyState      : LinearLayout
    private lateinit var emptyText       : TextView
    private lateinit var fab             : View

    // ── Action bar for multi-select ────────────────────────────────────────
    private lateinit var selectionBar    : HorizontalScrollView   // shown while selecting
    private lateinit var selectionCount  : TextView
    private lateinit var btnSelCopy      : MaterialButton
    private lateinit var btnSelMove      : MaterialButton
    private lateinit var btnSelDelete    : MaterialButton
    private lateinit var btnSelShare     : MaterialButton
    private lateinit var btnSelCompress  : MaterialButton
    private lateinit var btnSelCancel    : MaterialButton

    // ── State ──────────────────────────────────────────────────────────────
    private var currentDir       : File    = Environment.getExternalStorageDirectory()
    private var isGridLayout     : Boolean = false
    private var sortMode         : SortMode = SortMode.NAME_ASC
    private var searchQuery      : String  = ""
    private var allFiles         : List<File> = emptyList()
    private var showHiddenFiles  : Boolean = false

    // ── Multi-select state ─────────────────────────────────────────────────
    private var isSelectionMode  : Boolean = false
    private val selectedFiles    : MutableSet<File> = mutableSetOf()

    // ── ZIP navigation state ───────────────────────────────────────────────
    // While inside a ZIP, we track the internal path (like "folder/subfolder/")
    private var currentZipFile   : File?   = null   // the original ZIP
    private var zipInternalPath  : String  = ""     // the path inside the ZIP
    private var zipEntries       : List<ZipEntryInfo> = emptyList()

    data class ZipEntryInfo(
        val name: String,          // the displayed name
        val fullPath: String,      // the full path inside the ZIP
        val isDirectory: Boolean,
        val size: Long,
        val compressedSize: Long
    )

    // ── Storage roots ──────────────────────────────────────────────────────
    private val storageRoots: List<StorageRoot> by lazy { detectStorageRoots() }

    // ─────────────────────────────────────────────────────────────────────
    override fun onCreateView(inflater: LayoutInflater, c: ViewGroup?, s: Bundle?): View =
        inflater.inflate(R.layout.fragment_file_manager, c, false)

    override fun onViewCreated(v: View, s: Bundle?) {
        super.onViewCreated(v, s)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            !android.os.Environment.isExternalStorageManager()) {
            MaterialAlertDialogBuilder(requireContext())
                .setTitle(getString(R.string.file_access_permission))
                .setMessage(getString(R.string.manage_external_storage_required))
                .setPositiveButton(getString(R.string.grant)) { _, _ ->
                    startActivity(Intent(
                        Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                        android.net.Uri.parse("package:" + requireContext().packageName)
                    ))
                }
                .setNegativeButton(getString(R.string.skip), null)
                .show()
        }

        bindViews(v)
        setupStorageChips()
        setupSearchBar()
        setupSortBtn()
        setupViewToggle()
        setupSwipeRefresh()
        setupFab()
        setupSelectionBar()
        navigate(currentDir)
    }

    // ── Bind ──────────────────────────────────────────────────────────────
    private fun bindViews(v: View) {
        searchBar       = v.findViewById(R.id.fm_search_bar)
        breadcrumb      = v.findViewById(R.id.fm_breadcrumb)
        breadcrumbScroll= v.findViewById(R.id.fm_breadcrumb_scroll)
        storageChips    = v.findViewById(R.id.fm_storage_chips)
        sortBtn         = v.findViewById(R.id.fm_sort_btn)
        viewToggle      = v.findViewById(R.id.fm_view_toggle)
        itemCount       = v.findViewById(R.id.fm_item_count)
        recycler        = v.findViewById(R.id.fm_recycler)
        swipeRefresh    = v.findViewById(R.id.fm_swipe_refresh)
        emptyState      = v.findViewById(R.id.fm_empty_state)
        emptyText       = v.findViewById(R.id.fm_empty_text)
        fab             = v.findViewById(R.id.fm_fab)

        selectionBar    = v.findViewById(R.id.fm_selection_bar)
        selectionCount  = v.findViewById(R.id.fm_selection_count)
        btnSelCopy      = v.findViewById(R.id.fm_sel_copy)
        btnSelMove      = v.findViewById(R.id.fm_sel_move)
        btnSelDelete    = v.findViewById(R.id.fm_sel_delete)
        btnSelShare     = v.findViewById(R.id.fm_sel_share)
        btnSelCompress  = v.findViewById(R.id.fm_sel_compress)
        btnSelCancel    = v.findViewById(R.id.fm_sel_cancel)
    }

    // ── Selection Bar Setup ────────────────────────────────────────────────
    private fun setupSelectionBar() {
        btnSelCancel.setOnClickListener { exitSelectionMode() }

        btnSelDelete.setOnClickListener {
            if (selectedFiles.isEmpty()) return@setOnClickListener
            val count = selectedFiles.size
            MaterialAlertDialogBuilder(requireContext())
                .setTitle(getString(R.string.delete_n_items, count))
                .setMessage(getString(R.string.action_cannot_be_undone))
                .setPositiveButton(getString(R.string.delete)) { _, _ ->
                    selectedFiles.forEach { f ->
                        if (f.isDirectory) f.deleteRecursively() else f.delete()
                    }
                    exitSelectionMode()
                    refreshList()
                    snack(getString(R.string.deleted_n_items, count))
                }
                .setNegativeButton(getString(R.string.cancel), null)
                .show()
        }

        btnSelCopy.setOnClickListener {
            val files = selectedFiles.toList()
            if (files.isEmpty()) return@setOnClickListener
            showFolderPicker(getString(R.string.copy_n_items_to, files.size)) { dest ->
                Thread {
                    var success = 0
                    files.forEach { f ->
                        try { f.copyRecursively(File(dest, f.name), overwrite = true); success++ }
                        catch (_: Exception) {}
                    }
                    requireActivity().runOnUiThread {
                        exitSelectionMode()
                        snack(getString(R.string.copied_x_of_y, success, files.size))
                    }
                }.start()
            }
        }

        btnSelMove.setOnClickListener {
            val files = selectedFiles.toList()
            if (files.isEmpty()) return@setOnClickListener
            showFolderPicker(getString(R.string.move_n_items_to, files.size)) { dest ->
                Thread {
                    var success = 0
                    files.forEach { f ->
                        try {
                            val target = File(dest, f.name)
                            if (f.renameTo(target)) success++
                            else { f.copyRecursively(target, overwrite = true); f.deleteRecursively() }
                        } catch (_: Exception) {}
                    }
                    requireActivity().runOnUiThread {
                        exitSelectionMode()
                        refreshList()
                        snack(getString(R.string.moved_x_of_y, success, files.size))
                    }
                }.start()
            }
        }

        btnSelShare.setOnClickListener {
            val files = selectedFiles.toList()
            if (files.isEmpty()) return@setOnClickListener
            if (files.size == 1) { shareFile(files[0]); return@setOnClickListener }
            val uris = ArrayList<Uri>()
            files.forEach { f ->
                try {
                    uris.add(FileProvider.getUriForFile(requireContext(),
                        "${requireContext().packageName}.provider", f))
                } catch (_: Exception) {}
            }
            if (uris.isEmpty()) { snack(getString(R.string.share_failed)); return@setOnClickListener }
            startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND_MULTIPLE).apply {
                type = "*/*"
                putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }, getString(R.string.share_n_files, files.size)))
        }

        btnSelCompress.setOnClickListener {
            val files = selectedFiles.toList()
            if (files.isEmpty()) return@setOnClickListener
            val zipName = if (files.size == 1) "${files[0].nameWithoutExtension}.zip"
                          else "${getString(R.string.compressed_prefix)}_${SimpleDateFormat("yyyyMMdd_HHmm", Locale.getDefault()).format(Date())}.zip"
            val zipFile = File(currentDir, zipName)
            doCompressMultiple(files, zipFile)
            exitSelectionMode()
        }
    }

    private fun enterSelectionMode(firstFile: File) {
        isSelectionMode = true
        selectedFiles.clear()
        selectedFiles.add(firstFile)
        selectionBar.visibility = View.VISIBLE
        fab.visibility = View.GONE
        updateSelectionCount()
        recycler.adapter?.notifyDataSetChanged()
    }

    private fun exitSelectionMode() {
        isSelectionMode = false
        selectedFiles.clear()
        selectionBar.visibility = View.GONE
        fab.visibility = View.VISIBLE
        updateSelectionCount()
        recycler.adapter?.notifyDataSetChanged()
    }

    private fun toggleSelection(f: File) {
        if (selectedFiles.contains(f)) selectedFiles.remove(f)
        else selectedFiles.add(f)
        updateSelectionCount()
        if (selectedFiles.isEmpty()) exitSelectionMode()
        recycler.adapter?.notifyDataSetChanged()
    }

    private fun updateSelectionCount() {
        selectionCount.text = getString(R.string.n_selected, selectedFiles.size)
    }

    // ── Storage chips ──────────────────────────────────────────────────────
    private fun setupStorageChips() {
        storageChips.removeAllViews()
        storageRoots.forEachIndexed { i, root ->
            val chip = Chip(requireContext()).apply {
                text = root.label
                isCheckable = true
                isChecked = i == 0
                setOnCheckedChangeListener { _, checked ->
                    if (checked) { exitZipMode(); currentDir = root.file; navigate(root.file) }
                }
            }
            storageChips.addView(chip)
        }
    }

    // ── Search ─────────────────────────────────────────────────────────────
    private fun setupSearchBar() {
        searchBar.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                R.id.fm_action_show_hidden -> {
                    showHiddenFiles = !showHiddenFiles
                    item.title = if (showHiddenFiles) getString(R.string.hide_hidden_files) else getString(R.string.show_hidden_files)
                    refreshList(); true
                }
                R.id.fm_action_select_all -> {
                    if (!isSelectionMode) {
                        val files = allFiles
                        if (files.isEmpty()) { snack(getString(R.string.no_files_found)); return@setOnMenuItemClickListener true }
                        enterSelectionMode(files[0])
                        selectedFiles.addAll(files)
                        updateSelectionCount()
                        recycler.adapter?.notifyDataSetChanged()
                    } else {
                        selectedFiles.addAll(allFiles)
                        updateSelectionCount()
                        recycler.adapter?.notifyDataSetChanged()
                    }
                    true
                }
                else -> false
            }
        }
        searchBar.post { attachSearchListener(searchBar) }
    }

    private fun attachSearchListener(vg: android.view.ViewGroup) {
        for (i in 0 until vg.childCount) {
            val child = vg.getChildAt(i)
            if (child is android.widget.EditText) {
                child.addTextChangedListener(object : android.text.TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, st: Int, c: Int, a: Int) {}
                    override fun onTextChanged(s: CharSequence?, st: Int, b: Int, c: Int) {
                        searchQuery = s?.toString()?.trim() ?: ""
                        refreshList()
                    }
                    override fun afterTextChanged(s: android.text.Editable?) {}
                })
                return
            }
            if (child is android.view.ViewGroup) attachSearchListener(child)
        }
    }

    // ── Sort ───────────────────────────────────────────────────────────────
    private fun setupSortBtn() { sortBtn.setOnClickListener { showSortDialog() } }

    private fun showSortDialog() {
        val options = resources.getStringArray(R.array.sort_options)
        MaterialAlertDialogBuilder(requireContext())
            .setTitle(getString(R.string.sort))
            .setSingleChoiceItems(options, sortMode.ordinal) { d, which ->
                sortMode = SortMode.entries[which]
                sortBtn.text = options[which].substringBefore("(").trim()
                d.dismiss(); refreshList()
            }.show()
    }

    // ── View toggle ────────────────────────────────────────────────────────
    private fun setupViewToggle() {
        updateToggleIcon()
        viewToggle.setOnClickListener { isGridLayout = !isGridLayout; updateToggleIcon(); rebuildAdapter() }
    }

    private fun updateToggleIcon() {
        viewToggle.setIconResource(if (isGridLayout) R.drawable.ic_view_list else R.drawable.ic_apps_menu)
    }

    // ── SwipeRefresh ──────────────────────────────────────────────────────
    private fun setupSwipeRefresh() {
        swipeRefresh.setOnRefreshListener { refreshList(); swipeRefresh.isRefreshing = false }
    }

    // ── FAB ───────────────────────────────────────────────────────────────
    private fun setupFab() {
        fab.visibility = View.VISIBLE
        fab.setOnClickListener { showCreateFolderDialog() }
    }

    private fun showCreateFolderDialog() {
        if (currentZipFile != null) { snack(getString(R.string.cannot_create_folder_in_zip)); return }
        val inputLayout = TextInputLayout(requireContext(), null,
            com.google.android.material.R.attr.textInputOutlinedStyle).apply {
            hint = getString(R.string.folder_name)
            val p = (16 * resources.displayMetrics.density).toInt()
            setPadding(p, p, p, 0)
            boxBackgroundMode = TextInputLayout.BOX_BACKGROUND_OUTLINE
        }
        val input = TextInputEditText(requireContext()).apply { isSingleLine = true }
        inputLayout.addView(input)
        MaterialAlertDialogBuilder(requireContext())
            .setTitle(getString(R.string.new_folder)).setView(inputLayout)
            .setPositiveButton(getString(R.string.create)) { _, _ ->
                val name = input.text.toString().trim()
                if (name.isBlank()) { snack(getString(R.string.name_is_empty_short)); return@setPositiveButton }
                val newDir = File(currentDir, name)
                if (newDir.mkdirs()) { snack(getString(R.string.folder_created)); refreshList() }
                else snack(getString(R.string.creation_failed))
            }.setNegativeButton(getString(R.string.cancel), null).show()
    }

    // ══════════════════════════════════════════════════════════════════════
    // ZIP NAVIGATION — the new core feature
    // ══════════════════════════════════════════════════════════════════════

    /** Opens a ZIP as a folder — we start from the root */
    private fun enterZipMode(zipFile: File) {
        currentZipFile  = zipFile
        zipInternalPath = ""
        loadZipEntries()
    }

    /** Exits ZIP mode entirely */
    private fun exitZipMode() {
        currentZipFile  = null
        zipInternalPath = ""
        zipEntries      = emptyList()
    }

    /** Loads the ZIP entries for the current path */
    private fun loadZipEntries() {
        val zipFile = currentZipFile ?: return
        try {
            ZipFile(zipFile).use { zip ->
                val prefix = zipInternalPath  // e.g.: "" or "folder/" or "a/b/"
                val seen   = mutableSetOf<String>()
                val result = mutableListOf<ZipEntryInfo>()

                zip.entries().asSequence().forEach { entry ->
                    val name = entry.name
                    if (!name.startsWith(prefix)) return@forEach
                    val relative = name.removePrefix(prefix)
                    if (relative.isEmpty()) return@forEach

                    val slash = relative.indexOf('/')
                    if (slash == -1) {
                        // A direct file at this level
                        if (seen.add(relative)) {
                            result.add(ZipEntryInfo(
                                name           = relative,
                                fullPath       = name,
                                isDirectory    = false,
                                size           = entry.size,
                                compressedSize = entry.compressedSize
                            ))
                        }
                    } else {
                        // A folder
                        val dirName = relative.substring(0, slash)
                        if (seen.add(dirName)) {
                            result.add(ZipEntryInfo(
                                name           = dirName,
                                fullPath       = prefix + dirName + "/",
                                isDirectory    = true,
                                size           = 0L,
                                compressedSize = 0L
                            ))
                        }
                    }
                }
                zipEntries = result.sortedWith(compareBy({ !it.isDirectory }, { it.name.lowercase() }))
            }
        } catch (e: Exception) {
            snack(getString(R.string.cannot_read_zip, e.message ?: ""))
            exitZipMode()
        }
        updateBreadcrumb()
        rebuildZipAdapter()
    }

    /** Browses a folder inside the ZIP */
    private fun navigateInsideZip(dirPath: String) {
        zipInternalPath = dirPath
        loadZipEntries()
    }

    /** Goes up one level inside the ZIP */
    private fun navigateUpInsideZip(): Boolean {
        if (zipInternalPath.isEmpty()) {
            // We've exited the ZIP entirely
            exitZipMode()
            navigate(currentDir)
            return true
        }
        val parent = zipInternalPath.trimEnd('/').substringBeforeLast('/', "")
        zipInternalPath = if (parent.isEmpty()) "" else "$parent/"
        loadZipEntries()
        return true
    }

    // ── ZIP Adapter ────────────────────────────────────────────────────────
    private fun rebuildZipAdapter() {
        val entries = if (searchQuery.isBlank()) zipEntries
                      else zipEntries.filter { it.name.contains(searchQuery, ignoreCase = true) }

        val isEmpty = entries.isEmpty()
        emptyState.visibility = if (isEmpty) View.VISIBLE else View.GONE
        recycler.visibility   = if (isEmpty) View.INVISIBLE else View.VISIBLE
        emptyText.text        = getString(R.string.no_files_here)
        itemCount.text        = getString(R.string.n_items, entries.size)

        recycler.layoutManager = if (isGridLayout) GridLayoutManager(requireContext(), 3)
                                 else LinearLayoutManager(requireContext())
        recycler.adapter = ZipAdapter(entries)
    }

    inner class ZipAdapter(private val items: List<ZipEntryInfo>) :
        RecyclerView.Adapter<ZipAdapter.VH>() {

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val icon   : ImageView    = v.findViewById(R.id.fm_item_icon)
            val thumb  : ImageView    = v.findViewById(R.id.fm_item_thumbnail)
            val name   : TextView     = v.findViewById(R.id.fm_item_name)
            val size   : TextView     = v.findViewById(R.id.fm_item_size)
            val date   : TextView     = v.findViewById(R.id.fm_item_date)
            val menu   : MaterialButton = v.findViewById(R.id.fm_item_menu)
        }

        override fun getItemViewType(pos: Int) = if (isGridLayout) 1 else 0

        override fun onCreateViewHolder(p: ViewGroup, t: Int): VH {
            val layout = if (t == 1) R.layout.item_fm_file_grid else R.layout.item_fm_file_list
            return VH(LayoutInflater.from(p.context).inflate(layout, p, false))
        }

        override fun getItemCount() = items.size

        override fun onBindViewHolder(h: VH, pos: Int) {
            val entry = items[pos]
            h.name.text = entry.name
            h.thumb.visibility = View.GONE
            h.icon.visibility  = View.VISIBLE
            h.date.text = getString(R.string.inside_zip)

            if (entry.isDirectory) {
                h.icon.setImageResource(R.drawable.ic_zip) // ZIP-folder icon
                h.size.text = getString(R.string.folder)
                h.itemView.setOnClickListener { navigateInsideZip(entry.fullPath) }
            } else {
                h.icon.setImageResource(iconForExt(entry.name.substringAfterLast('.', "").lowercase()))
                h.size.text = Formatter.formatShortFileSize(requireContext(), entry.size)
                h.itemView.setOnClickListener { /* nothing, or show text */ }
            }

            // The context menu for each entry
            h.menu.setOnClickListener { showZipEntryOptions(entry, h.menu) }
            h.itemView.setOnLongClickListener { showZipEntryOptions(entry, h.menu); true }
        }
    }

    /** The options menu inside a ZIP */
    private fun showZipEntryOptions(entry: ZipEntryInfo, anchor: View) {
        val popup = PopupMenu(requireContext(), anchor)
        popup.menu.apply {
            if (!entry.isDirectory) add(0, 1, 0, getString(R.string.extract_here))
            add(0, 2, 1, getString(R.string.extract_to))
            if (!entry.isDirectory) add(0, 3, 2, getString(R.string.copy_internal_path))
        }
        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> extractZipEntry(entry, currentDir)
                2 -> showFolderPicker(getString(R.string.extract_to)) { dest -> extractZipEntry(entry, dest) }
                3 -> {
                    val cm = requireContext().getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    cm.setPrimaryClip(ClipData.newPlainText("zipPath", entry.fullPath))
                    snack(getString(R.string.path_copied))
                }
            }
            true
        }
        popup.show()
    }

    /** Extracts a single entry (file or folder) from the ZIP */
    private fun extractZipEntry(entry: ZipEntryInfo, destDir: File) {
        val zipFile = currentZipFile ?: return
        val progress = android.app.ProgressDialog(requireContext()).apply {
            setTitle(getString(R.string.extracting))
            setMessage(entry.name)
            isIndeterminate = true
            setCancelable(false)
            show()
        }
        Thread {
            try {
                val destCanon = destDir.canonicalPath
                ZipFile(zipFile).use { zip ->
                    zip.entries().asSequence()
                        .filter { it.name.startsWith(entry.fullPath) }
                        .forEach { e ->
                            val relative = e.name.removePrefix(
                                if (entry.isDirectory) entry.fullPath.substringBeforeLast('/', entry.fullPath) + "/"
                                else entry.fullPath.substringBeforeLast('/').let { if (it.isEmpty()) "" else "$it/" }
                            )
                            val outFile = File(destDir, relative)
                            if (!outFile.canonicalPath.startsWith(destCanon)) return@forEach
                            if (e.isDirectory) { outFile.mkdirs(); return@forEach }
                            outFile.parentFile?.mkdirs()
                            zip.getInputStream(e).use { input -> outFile.outputStream().use { input.copyTo(it) } }
                        }
                }
                requireActivity().runOnUiThread {
                    progress.dismiss()
                    snack(getString(R.string.extracted_to, destDir.name))
                }
            } catch (e: Exception) {
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.failed_colon, e.message ?: "")) }
            }
        }.start()
    }

    // ══════════════════════════════════════════════════════════════════════
    // FOLDER PICKER — for choosing the copy/move/extract destination
    // ══════════════════════════════════════════════════════════════════════
    private fun showFolderPicker(title: String, onPicked: (File) -> Unit) {
        val ctx = requireContext()
        var pickerDir = storageRoots.firstOrNull()?.file ?: Environment.getExternalStorageDirectory()

        // Build the dialog
        val dialogView = LayoutInflater.from(ctx).inflate(R.layout.dialog_folder_picker, null)
        val pickerBreadcrumb = dialogView.findViewById<LinearLayout>(R.id.fp_breadcrumb)
        val pickerBreadcrumbScroll = dialogView.findViewById<HorizontalScrollView>(R.id.fp_breadcrumb_scroll)
        val pickerRecycler  = dialogView.findViewById<RecyclerView>(R.id.fp_recycler)
        val pickerCurrent   = dialogView.findViewById<TextView>(R.id.fp_current_path)

        var dialog: AlertDialog? = null

        fun refreshPicker() {
            val dirs = (pickerDir.listFiles()?.filter { it.isDirectory && !it.name.startsWith(".") } ?: emptyList())
                .sortedBy { it.name.lowercase() }

            pickerCurrent?.text = pickerDir.absolutePath

            // Breadcrumb
            pickerBreadcrumb?.removeAllViews()
            val root = storageRoots.firstOrNull { pickerDir.canonicalPath.startsWith(it.file.canonicalPath) }
            if (root != null) {
                val parts = mutableListOf<Pair<String, File>>()
                var f: File? = pickerDir
                while (f != null && f.canonicalPath != root.file.canonicalPath) {
                    parts.add(0, Pair(f.name, f)); f = f.parentFile
                }
                parts.add(0, Pair(root.label, root.file))
                parts.forEachIndexed { i, (label, file) ->
                    if (i > 0) {
                        val sep = TextView(ctx).apply { text = " / "; textSize = 11f }
                        pickerBreadcrumb?.addView(sep)
                    }
                    val btn = TextView(ctx).apply {
                        text = label; textSize = 12f
                        val isLast = i == parts.lastIndex
                        setTextColor(if (isLast) 0xFFFFFFFF.toInt() else 0xFF888888.toInt())
                        val p = (4 * resources.displayMetrics.density).toInt()
                        setPadding(p, p, p, p)
                        if (!isLast) setOnClickListener { pickerDir = file; refreshPicker() }
                    }
                    pickerBreadcrumb?.addView(btn)
                }
                pickerBreadcrumbScroll?.post { pickerBreadcrumbScroll.fullScroll(View.FOCUS_RIGHT) }
            }

            pickerRecycler?.layoutManager = LinearLayoutManager(ctx)
            pickerRecycler?.adapter = object : RecyclerView.Adapter<RecyclerView.ViewHolder>() {
                inner class DVH(v: View) : RecyclerView.ViewHolder(v) {
                    val name: TextView = v.findViewById(android.R.id.text1)
                }
                override fun onCreateViewHolder(p: ViewGroup, t: Int) = DVH(
                    LayoutInflater.from(ctx).inflate(android.R.layout.simple_list_item_1, p, false))
                override fun getItemCount() = dirs.size
                override fun onBindViewHolder(h: RecyclerView.ViewHolder, pos: Int) {
                    val d = dirs[pos]
                    (h as DVH).name.text = d.name
                    h.itemView.setOnClickListener { pickerDir = d; refreshPicker() }
                }
            }
        }

        refreshPicker()

        dialog = MaterialAlertDialogBuilder(ctx)
            .setTitle(title)
            .setView(dialogView)
            .setPositiveButton(getString(R.string.choose_here)) { _, _ -> onPicked(pickerDir) }
            .setNegativeButton(getString(R.string.cancel), null)
            .create()
        dialog.show()
    }

    // ══════════════════════════════════════════════════════════════════════
    // NORMAL NAVIGATION
    // ══════════════════════════════════════════════════════════════════════
    fun navigate(dir: File) {
        exitZipMode()
        exitSelectionMode()
        currentDir   = dir
        searchQuery  = ""
        refreshList()
        updateBreadcrumb()
    }

    fun navigateUp(): Boolean {
        // Inside a ZIP
        if (currentZipFile != null) return navigateUpInsideZip()

        // Selection mode — cancel it first
        if (isSelectionMode) { exitSelectionMode(); return true }

        val parent = currentDir.parentFile
        val root   = storageRoots.firstOrNull { currentDir.canonicalPath.startsWith(it.file.canonicalPath) }?.file
        if (parent != null && parent.canRead() && currentDir != root) {
            navigate(parent); return true
        }
        return false
    }

    private fun updateBreadcrumb() {
        breadcrumb.removeAllViews()

        // Inside a ZIP — we show: Storage / zipName / internal path
        if (currentZipFile != null) {
            val zipFile = currentZipFile!!
            val parts = mutableListOf<Pair<String, (() -> Unit)?>>()
            parts.add(Pair(zipFile.name) { exitZipMode(); navigate(currentDir) })

            if (zipInternalPath.isNotEmpty()) {
                val segments = zipInternalPath.trimEnd('/').split('/')
                var pathSoFar = ""
                segments.forEach { seg ->
                    pathSoFar += "$seg/"
                    val captured = pathSoFar
                    parts.add(Pair(seg) { navigateInsideZip(captured) })
                }
            }

            parts.forEachIndexed { i, (label, onClick) ->
                if (i > 0) breadcrumb.addView(TextView(requireContext()).apply {
                    text = " / "; textSize = 12f; setTextColor(0xFF888888.toInt())
                })
                breadcrumb.addView(TextView(requireContext()).apply {
                    text = label; textSize = 13f
                    val isLast = i == parts.lastIndex
                    setTextColor(if (isLast) 0xFFFFFFFF.toInt() else 0xFF888888.toInt())
                    val p = (6 * resources.displayMetrics.density).toInt()
                    setPadding(p, p / 2, p, p / 2)
                    if (!isLast && onClick != null) setOnClickListener { onClick() }
                })
            }
            breadcrumbScroll.post { breadcrumbScroll.fullScroll(View.FOCUS_RIGHT) }
            return
        }

        // Normal
        val root = storageRoots.firstOrNull {
            currentDir.canonicalPath.startsWith(it.file.canonicalPath)
        } ?: return

        val parts = mutableListOf<Pair<String, File>>()
        var f: File? = currentDir
        while (f != null && f.canonicalPath != root.file.canonicalPath) {
            parts.add(0, Pair(f.name, f)); f = f.parentFile
        }
        parts.add(0, Pair(root.label, root.file))

        parts.forEachIndexed { i, (label, file) ->
            if (i > 0) breadcrumb.addView(TextView(requireContext()).apply {
                text = " / "; textSize = 12f; setTextColor(0xFF888888.toInt())
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

    // ── List ───────────────────────────────────────────────────────────────
    private fun refreshList() {
        if (currentZipFile != null) { rebuildZipAdapter(); return }

        val raw = (currentDir.listFiles()?.toList() ?: emptyList())
            .let { if (showHiddenFiles) it else it.filter { f -> !f.name.startsWith(".") } }
        allFiles = sortFiles(raw)
        val filtered = if (searchQuery.isBlank()) allFiles
                       else allFiles.filter { it.name.contains(searchQuery, ignoreCase = true) }

        val isEmpty = filtered.isEmpty()
        emptyState.visibility   = if (isEmpty) View.VISIBLE else View.GONE
        recycler.visibility     = if (isEmpty) View.INVISIBLE else View.VISIBLE
        swipeRefresh.visibility = View.VISIBLE
        emptyText.text = if (currentDir.canRead()) getString(R.string.folder_empty) else getString(R.string.no_access_permission)
        itemCount.text = getString(R.string.n_items, filtered.size)

        rebuildAdapter(filtered)
    }

    private fun rebuildAdapter(files: List<File>? = null) {
        val data = files ?: run {
            val raw = allFiles.ifEmpty { currentDir.listFiles()?.toList() ?: emptyList() }
            if (searchQuery.isBlank()) raw
            else raw.filter { it.name.contains(searchQuery, ignoreCase = true) }
        }
        recycler.layoutManager = if (isGridLayout) GridLayoutManager(requireContext(), 3)
                                 else LinearLayoutManager(requireContext())
        recycler.adapter = FmAdapter(data)
    }

    private fun sortFiles(list: List<File>): List<File> {
        val dirs  = list.filter { it.isDirectory }
        val files = list.filter { it.isFile }
        fun sort(l: List<File>) = when (sortMode) {
            SortMode.NAME_ASC  -> l.sortedBy { it.name.lowercase() }
            SortMode.NAME_DESC -> l.sortedByDescending { it.name.lowercase() }
            SortMode.SIZE_ASC  -> l.sortedBy { it.length() }
            SortMode.SIZE_DESC -> l.sortedByDescending { it.length() }
            SortMode.DATE_DESC -> l.sortedByDescending { it.lastModified() }
            SortMode.DATE_ASC  -> l.sortedBy { it.lastModified() }
        }
        return sort(dirs) + sort(files)
    }

    // ── Main Adapter ───────────────────────────────────────────────────────
    inner class FmAdapter(private val items: List<File>) :
        RecyclerView.Adapter<FmAdapter.VH>() {

        override fun getItemViewType(pos: Int) = if (isGridLayout) 1 else 0

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val icon      : ImageView      = v.findViewById(R.id.fm_item_icon)
            val thumbnail : ImageView      = v.findViewById(R.id.fm_item_thumbnail)
            val name      : TextView       = v.findViewById(R.id.fm_item_name)
            val size      : TextView       = v.findViewById(R.id.fm_item_size)
            val date      : TextView       = v.findViewById(R.id.fm_item_date)
            val menuBtn   : MaterialButton = v.findViewById(R.id.fm_item_menu)
        }

        override fun onCreateViewHolder(p: ViewGroup, t: Int): VH {
            val layout = if (t == 1) R.layout.item_fm_file_grid else R.layout.item_fm_file_list
            return VH(LayoutInflater.from(p.context).inflate(layout, p, false))
        }

        override fun getItemCount() = items.size

        override fun onBindViewHolder(h: VH, pos: Int) {
            val f = items[pos]
            h.name.text = f.name
            h.date.text = SimpleDateFormat("yyyy/MM/dd", Locale.getDefault()).format(Date(f.lastModified()))

            // ── Visually highlight the selected item ────────────────────────────────────
            val isSelected = selectedFiles.contains(f)
            h.itemView.alpha = if (isSelectionMode && !isSelected) 0.5f else 1f
            (h.itemView as? com.google.android.material.card.MaterialCardView)?.apply {
                strokeWidth = if (isSelected) (2 * resources.displayMetrics.density).toInt() else 0
                strokeColor = if (isSelected) 0xFF6200EE.toInt() else 0
            }

            if (f.isDirectory) {
                val count = f.listFiles()?.size ?: 0
                h.size.text = getString(R.string.n_elements, count)
                h.icon.setImageResource(R.drawable.ic_folder)
                h.thumbnail.visibility = View.GONE
                h.icon.visibility = View.VISIBLE
            } else {
                h.size.text = Formatter.formatShortFileSize(requireContext(), f.length())
                loadFileIcon(f, h)
            }

            // ── Tap: select or open ────────────────────────────────────
            h.itemView.setOnClickListener {
                if (isSelectionMode) {
                    toggleSelection(f)
                } else {
                    if (f.isDirectory) navigate(f)
                    else openFile(f)
                }
            }

            // ── Long press: enter selection mode ───────────────────────
            h.itemView.setOnLongClickListener {
                if (!isSelectionMode) enterSelectionMode(f)
                else toggleSelection(f)
                true
            }

            h.menuBtn.setOnClickListener {
                if (isSelectionMode) toggleSelection(f)
                else showFileOptions(f, h.menuBtn)
            }
        }

        private fun loadFileIcon(f: File, h: VH) {
            val ext = f.extension.lowercase()

            h.thumbnail.setImageBitmap(null)
            h.thumbnail.visibility = View.GONE
            h.icon.visibility = View.VISIBLE
            h.icon.setImageResource(iconForExt(ext))

            if (com.youki.dex.utils.ThumbnailLoader.isPreviewable(ext)) {
                com.youki.dex.utils.ThumbnailLoader.load(f, h.itemView) { bmp ->
                    h.thumbnail.setImageBitmap(bmp)
                    h.thumbnail.visibility = View.VISIBLE
                    h.icon.visibility = View.GONE
                }
            }
        }
    }

    // ── File icon mapping ──────────────────────────────────────────────────
    private fun iconForExt(ext: String) = when (ext) {
        "mp4","mkv","webm","mov","3gp","avi"        -> R.drawable.ic_video_file
        "mp3","flac","aac","ogg","wav","m4a"        -> R.drawable.ic_audio_file
        "jpg","jpeg","png","gif","webp","bmp","svg" -> R.drawable.ic_image
        "pdf"                                       -> R.drawable.ic_pdf
        "zip","rar","7z","tar","gz"                 -> R.drawable.ic_zip
        "ttf","otf","woff","woff2"                  -> R.drawable.ic_font
        "apk"                                       -> R.drawable.ic_apk
        "txt","log","md","json","xml","csv"         -> R.drawable.ic_text_file
        else                                        -> R.drawable.ic_file
    }

    // ── File options (single file) ─────────────────────────────────────────
    private fun showFileOptions(f: File, anchor: View) {
        val ext    = f.extension.lowercase()
        val isZip  = ext == "zip"
        val isVideo= ext in setOf("mp4","mkv","webm","mov","3gp","avi")
        val isAudio= ext in setOf("mp3","flac","aac","ogg","wav","m4a","opus")
        val isText = ext in setOf("txt","log","md","json","xml","csv","html","js","kt","py","java","sh","ini","cfg","yaml","yml")
        val isApk  = ext == "apk"

        val popup = PopupMenu(requireContext(), anchor)
        popup.menu.apply {
            if (f.isDirectory)  add(0, 10, 0, getString(R.string.open))
            if (isZip)          add(0, 20, 0, getString(R.string.open_as_folder))      // new
            if (isZip)          add(0, 21, 1, getString(R.string.extract_here))
            if (isZip)          add(0, 22, 2, getString(R.string.extract_to))
            if (isVideo)        add(0,  1, 3, getString(R.string.play_video))
            if (isAudio)        add(0, 12, 3, getString(R.string.play_audio))
            if (isText)         add(0, 13, 3, getString(R.string.view_text))
            if (!isZip)         add(0, 14, 4, getString(R.string.compress_to_zip))
            if (isApk)          add(0,  9, 4, getString(R.string.install))
            add(0,  4, 10, getString(R.string.rename))
            add(0,  5, 11, getString(R.string.copy_to))
            add(0,  6, 12, getString(R.string.move_to))
            add(0,  7, 13, getString(R.string.share))
            add(0,  8, 14, getString(R.string.copy_path))
            add(0, 15, 15, getString(R.string.properties))
            add(0, 99, 99, getString(R.string.delete))
        }
        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                20 -> enterZipMode(f)                                        // open the ZIP as a folder
                21 -> extractZip(f, currentDir, deleteAfter = false)
                22 -> showFolderPicker(getString(R.string.extract_to)) { dest -> extractZip(f, dest, deleteAfter = false) }
                1  -> playVideoInternal(f)
                2  -> showZipPreview(f)
                4  -> showRenameDialog(f)
                5  -> showFolderPicker(getString(R.string.copy_to)) { dest ->
                         Thread { try { f.copyRecursively(File(dest, f.name), overwrite = true)
                             requireActivity().runOnUiThread { snack(getString(R.string.copied)) } }
                             catch (e: Exception) { requireActivity().runOnUiThread { snack(getString(R.string.failed_colon, e.message ?: "")) } }
                         }.start() }
                6  -> showFolderPicker(getString(R.string.move_to)) { dest ->
                         Thread { try {
                             val target = File(dest, f.name)
                             if (!f.renameTo(target)) { f.copyRecursively(target, true); f.deleteRecursively() }
                             requireActivity().runOnUiThread { snack(getString(R.string.moved)); refreshList() } }
                             catch (e: Exception) { requireActivity().runOnUiThread { snack(getString(R.string.failed_colon, e.message ?: "")) } }
                         }.start() }
                7  -> shareFile(f)
                8  -> copyPath(f)
                9  -> installApk(f)
                10 -> if (f.isDirectory) navigate(f)
                12 -> playAudioInternal(f)
                13 -> showTextViewer(f)
                14 -> compressToZip(f)
                15 -> showFileProperties(f)
                99 -> confirmDelete(f)
            }
            true
        }
        popup.show()
    }

    // ── Open file ──────────────────────────────────────────────────────────
    private fun openFile(f: File) {
        val ext = f.extension.lowercase()
        when {
            ext == "zip" -> enterZipMode(f)                                   // ZIP → straight inside it
            ext in setOf("mp4","mkv","webm","mov","3gp","avi") -> { playVideoInternal(f); return }
            ext in setOf("mp3","flac","aac","ogg","wav","m4a","opus") -> { playAudioInternal(f); return }
            ext in setOf("txt","log","md","json","xml","csv","html","js","kt","py","java","sh","ini","cfg","yaml","yml") -> { showTextViewer(f); return }
            else -> {
                try {
                    val uri = FileProvider.getUriForFile(requireContext(), "${requireContext().packageName}.provider", f)
                    val mime = requireContext().contentResolver.getType(uri) ?: "*/*"
                    startActivity(Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(uri, mime); addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    })
                } catch (_: Exception) { snack(getString(R.string.no_app_to_open_file)) }
            }
        }
    }

    private fun playVideoInternal(f: File) = VideoUtils.play(requireContext(), f)

    private fun Int.dp(ctx: Context) = (this * ctx.resources.displayMetrics.density).toInt()

    // ── Audio Player ──────────────────────────────────────────────────────
    private var mediaPlayer: android.media.MediaPlayer? = null
    // FIX (Leaked Window): we track the dialog so we can close it in
    // onDestroyView. Without this, if the user leaves the screen while audio
    // is playing, the dialog stays hanging above the WindowManager window even
    // after the Fragment is destroyed — a possible "WindowLeaked" exception
    // plus a dead, unresponsive UI element.
    private var audioDialog: android.app.Dialog? = null

    private fun playAudioInternal(f: File) {
        mediaPlayer?.apply { if (isPlaying) stop(); release() }; mediaPlayer = null

        val uri = try { FileProvider.getUriForFile(requireContext(), "${requireContext().packageName}.provider", f) }
                  catch (_: Exception) { snack(getString(R.string.cannot_read_file)); return }
        val ctx = requireContext()
        val dialog = MaterialAlertDialogBuilder(ctx).create()
        val root = android.widget.LinearLayout(ctx).apply {
            orientation = android.widget.LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(24.dp(ctx), 24.dp(ctx), 24.dp(ctx), 16.dp(ctx))
        }
        val musicIcon = TextView(ctx).apply { text = "Audio"; textSize = 20f; gravity = Gravity.CENTER }
        val titleTv   = TextView(ctx).apply {
            text = f.nameWithoutExtension; textSize = 15f; gravity = Gravity.CENTER
            setTextColor(android.graphics.Color.WHITE); setPadding(0, 12.dp(ctx), 0, 4.dp(ctx))
            maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END
        }
        val timeTv    = TextView(ctx).apply {
            text = "0:00 / 0:00"; textSize = 12f; gravity = Gravity.CENTER
            setTextColor(0xFFAAAAAA.toInt()); setPadding(0, 0, 0, 8.dp(ctx))
        }
        val seekBar   = SeekBar(ctx).apply {
            layoutParams = android.widget.LinearLayout.LayoutParams(android.widget.LinearLayout.LayoutParams.MATCH_PARENT, android.widget.LinearLayout.LayoutParams.WRAP_CONTENT)
        }
        val controls  = android.widget.LinearLayout(ctx).apply {
            orientation = android.widget.LinearLayout.HORIZONTAL; gravity = Gravity.CENTER; setPadding(0, 12.dp(ctx), 0, 0)
        }
        val btnRew = ImageButton(ctx).apply { setImageResource(android.R.drawable.ic_media_rew); setBackgroundColor(android.graphics.Color.TRANSPARENT); layoutParams = android.widget.LinearLayout.LayoutParams(56.dp(ctx), 56.dp(ctx)) }
        val btnPP  = ImageButton(ctx).apply { setImageResource(android.R.drawable.ic_media_pause); setBackgroundColor(android.graphics.Color.TRANSPARENT); layoutParams = android.widget.LinearLayout.LayoutParams(64.dp(ctx), 64.dp(ctx)) }
        val btnFfw = ImageButton(ctx).apply { setImageResource(android.R.drawable.ic_media_ff); setBackgroundColor(android.graphics.Color.TRANSPARENT); layoutParams = android.widget.LinearLayout.LayoutParams(56.dp(ctx), 56.dp(ctx)) }
        controls.addView(btnRew); controls.addView(btnPP); controls.addView(btnFfw)
        root.addView(musicIcon); root.addView(titleTv); root.addView(timeTv); root.addView(seekBar); root.addView(controls)

        val mp = android.media.MediaPlayer().apply {
            try { setDataSource(ctx, uri); prepare() } catch (e: Exception) { snack(getString(R.string.cannot_play_audio)); return }
        }
        mediaPlayer = mp
        fun fmtTime(ms: Int): String { val s = ms / 1000; return "${s/60}:${"%02d".format(s%60)}" }
        seekBar.max = mp.duration; timeTv.text = "0:00 / ${fmtTime(mp.duration)}"
        val handler = android.os.Handler(android.os.Looper.getMainLooper())
        val updater = object : Runnable { override fun run() { if (mp.isPlaying) { seekBar.progress = mp.currentPosition; timeTv.text = "${fmtTime(mp.currentPosition)} / ${fmtTime(mp.duration)}"; handler.postDelayed(this, 500) } } }
        btnPP.setOnClickListener { if (mp.isPlaying) { mp.pause(); handler.removeCallbacks(updater); btnPP.setImageResource(android.R.drawable.ic_media_play) } else { mp.start(); handler.post(updater); btnPP.setImageResource(android.R.drawable.ic_media_pause) } }
        btnRew.setOnClickListener { mp.seekTo((mp.currentPosition - 10000).coerceAtLeast(0)) }
        btnFfw.setOnClickListener { mp.seekTo((mp.currentPosition + 10000).coerceAtMost(mp.duration)) }
        seekBar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener { override fun onProgressChanged(sb: SeekBar, p: Int, fromUser: Boolean) { if (fromUser) mp.seekTo(p) }; override fun onStartTrackingTouch(sb: SeekBar) {}; override fun onStopTrackingTouch(sb: SeekBar) {} })
        mp.setOnCompletionListener { btnPP.setImageResource(android.R.drawable.ic_media_play); seekBar.progress = 0; timeTv.text = "0:00 / ${fmtTime(mp.duration)}" }
        mp.start(); handler.post(updater)
        dialog.setView(root)
        dialog.setOnDismissListener { handler.removeCallbacks(updater); mp.apply { if (isPlaying) stop(); release() }; mediaPlayer = null; audioDialog = null }
        audioDialog = dialog
        dialog.show()
    }

    // ── Text Viewer ───────────────────────────────────────────────────────
    private fun showTextViewer(f: File) {
        if (f.length() > 512 * 1024) { snack(getString(R.string.file_too_large_to_view)); return }
        val content = try { f.readText(Charsets.UTF_8) } catch (_: Exception) { try { f.readText(Charsets.ISO_8859_1) } catch (_: Exception) { snack(getString(R.string.cannot_read_file)); return } }
        val ctx = requireContext()
        val sv  = ScrollView(ctx)
        val tv  = TextView(ctx).apply {
            text = content; textSize = 12f; typeface = android.graphics.Typeface.MONOSPACE
            setTextColor(0xFFCCCCCC.toInt()); setBackgroundColor(0xFF1A1A2E.toInt())
            val p = 16.dp(ctx); setPadding(p, p, p, p); setTextIsSelectable(true)
        }
        sv.addView(tv); sv.setBackgroundColor(0xFF1A1A2E.toInt())
        MaterialAlertDialogBuilder(ctx).setTitle(f.name).setView(sv)
            .setPositiveButton(getString(R.string.close), null)
            .setNeutralButton(getString(R.string.share)) { _, _ -> shareFile(f) }
            .show().also { d ->
                d.window?.setLayout((ctx.resources.displayMetrics.widthPixels * 0.92).toInt(),
                    (ctx.resources.displayMetrics.heightPixels * 0.8).toInt())
            }
    }

    // ── ZIP Preview (for a quick look from the options menu) ────────────────────
    private fun showZipPreview(zipFile: File) {
        val entries = try { ZipFile(zipFile).use { zip -> zip.entries().asSequence().map { Triple(it.name, it.size, it.isDirectory) }.toList() } }
                      catch (e: Exception) { snack(getString(R.string.cannot_read_zip, e.message ?: "")); return }
        val ctx = requireContext()
        val lv  = ListView(ctx)
        val items = entries.map { (name, size, isDir) -> "${if (isDir) "[${getString(R.string.folder)}]" else "[${getString(R.string.file)}]"}  $name${if (!isDir) "  (${Formatter.formatShortFileSize(ctx, size)})" else ""}" }
        lv.adapter = ArrayAdapter(ctx, android.R.layout.simple_list_item_1, items)
        val total = Formatter.formatShortFileSize(ctx, entries.filter { !it.third }.sumOf { it.second })
        MaterialAlertDialogBuilder(ctx).setTitle(zipFile.name)
            .setMessage(getString(R.string.n_items_total, entries.size, total)).setView(lv)
            .setPositiveButton(getString(R.string.open_as_folder)) { _, _ -> enterZipMode(zipFile) }
            .setNeutralButton(getString(R.string.extract_here)) { _, _ -> extractZip(zipFile, currentDir, false) }
            .setNegativeButton(getString(R.string.cancel), null).show()
    }

    // ── Extract ZIP ────────────────────────────────────────────────────────
    private fun extractZip(zipFile: File, destDir: File, deleteAfter: Boolean) {
        val ctx = requireContext()
        val progress = android.app.ProgressDialog(ctx).apply {
            setTitle(getString(R.string.extracting_ellipsis)); setMessage(zipFile.name)
            isIndeterminate = false; setProgressStyle(android.app.ProgressDialog.STYLE_HORIZONTAL)
            setCancelable(false); show()
        }
        Thread {
            try {
                destDir.mkdirs(); val destCanon = destDir.canonicalPath
                ZipFile(zipFile).use { zip ->
                    val all = zip.entries().asSequence().toList(); val total = all.size.coerceAtLeast(1)
                    all.forEachIndexed { idx, entry ->
                        val outFile = File(destDir, entry.name)
                        if (!outFile.canonicalPath.startsWith(destCanon + File.separator) && outFile.canonicalPath != destCanon) return@forEachIndexed
                        if (entry.isDirectory) { outFile.mkdirs(); return@forEachIndexed }
                        outFile.parentFile?.mkdirs()
                        zip.getInputStream(entry).use { input -> outFile.outputStream().use { input.copyTo(it) } }
                        requireActivity().runOnUiThread { progress.progress = ((idx + 1) * 100) / total }
                    }
                }
                if (deleteAfter) zipFile.delete()
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.extracted_to, destDir.name)); refreshList() }
            } catch (e: Exception) { requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.failed_colon, e.message ?: "")) } }
        }.start()
    }

    // ── Compress ───────────────────────────────────────────────────────────
    private fun compressToZip(f: File) {
        val zipName = "${f.nameWithoutExtension}.zip"
        val zipFile = File(f.parentFile ?: currentDir, zipName)
        if (zipFile.exists()) {
            MaterialAlertDialogBuilder(requireContext()).setTitle(getString(R.string.file_exists))
                .setMessage(getString(R.string.file_exists_overwrite_confirm, zipName))
                .setPositiveButton(getString(R.string.yes)) { _, _ -> doCompress(f, zipFile) }
                .setNegativeButton(getString(R.string.cancel), null).show()
            return
        }
        doCompress(f, zipFile)
    }

    private fun doCompress(source: File, zipFile: File) {
        val ctx = requireContext()
        val progress = android.app.ProgressDialog(ctx).apply {
            setTitle(getString(R.string.compressing_ellipsis)); setMessage(source.name); isIndeterminate = true; setCancelable(false); show()
        }
        Thread {
            try {
                ZipOutputStream(zipFile.outputStream().buffered()).use { zos ->
                    fun addToZip(file: File, entryPath: String) {
                        if (file.isDirectory) file.listFiles()?.forEach { addToZip(it, "$entryPath/${it.name}") }
                        else { zos.putNextEntry(ZipEntry(entryPath)); file.inputStream().use { it.copyTo(zos) }; zos.closeEntry() }
                    }
                    addToZip(source, source.name)
                }
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.compressed_colon, zipFile.name)); refreshList() }
            } catch (e: Exception) {
                zipFile.delete()
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.failed_colon, e.message ?: "")) }
            }
        }.start()
    }

    private fun doCompressMultiple(files: List<File>, zipFile: File) {
        val ctx = requireContext()
        val progress = android.app.ProgressDialog(ctx).apply {
            setTitle(getString(R.string.compressing_ellipsis)); setMessage(getString(R.string.n_files, files.size)); isIndeterminate = true; setCancelable(false); show()
        }
        Thread {
            try {
                ZipOutputStream(zipFile.outputStream().buffered()).use { zos ->
                    fun addToZip(file: File, entryPath: String) {
                        if (file.isDirectory) file.listFiles()?.forEach { addToZip(it, "$entryPath/${it.name}") }
                        else { zos.putNextEntry(ZipEntry(entryPath)); file.inputStream().use { it.copyTo(zos) }; zos.closeEntry() }
                    }
                    files.forEach { addToZip(it, it.name) }
                }
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.done_colon, zipFile.name)); refreshList() }
            } catch (e: Exception) {
                zipFile.delete()
                requireActivity().runOnUiThread { progress.dismiss(); snack(getString(R.string.failed_colon, e.message ?: "")) }
            }
        }.start()
    }

    // ── File Properties ────────────────────────────────────────────────────
    private fun showFileProperties(f: File) {
        val ctx = requireContext()
        val sdf = SimpleDateFormat("yyyy/MM/dd HH:mm:ss", Locale.getDefault())
        val size = if (f.isDirectory) {
            val bytes = f.walkTopDown().filter { it.isFile }.sumOf { it.length() }
            "${Formatter.formatShortFileSize(ctx, bytes)} (${f.walkTopDown().count { it.isFile }} ${getString(R.string.file)})"
        } else "${Formatter.formatShortFileSize(ctx, f.length())} (${f.length()} ${getString(R.string.bytes)})"
        val mime = if (f.isFile) try {
            val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.provider", f)
            ctx.contentResolver.getType(uri) ?: getString(R.string.unknown)
        } catch (_: Exception) { getString(R.string.unknown) } else getString(R.string.folder)
        val info = buildString {
            appendLine("${getString(R.string.prop_name)}:      ${f.name}"); appendLine("${getString(R.string.prop_path)}:     ${f.absolutePath}")
            appendLine("${getString(R.string.prop_size)}:      $size"); appendLine("${getString(R.string.prop_type)}:      $mime")
            appendLine("${getString(R.string.prop_modified)}:    ${sdf.format(Date(f.lastModified()))}")
            appendLine("${getString(R.string.prop_readable)}:    ${if (f.canRead()) getString(R.string.yes) else getString(R.string.no)}")
            appendLine("${getString(R.string.prop_writable)}:    ${if (f.canWrite()) getString(R.string.yes) else getString(R.string.no)}")
            if (f.isDirectory) appendLine("${getString(R.string.prop_items)}:   ${f.listFiles()?.size ?: 0} ${getString(R.string.direct_items)}")
        }
        val tv = TextView(ctx).apply {
            text = info; textSize = 13f; typeface = android.graphics.Typeface.MONOSPACE
            setTextColor(0xFFCCCCCC.toInt()); val p = 16.dp(ctx); setPadding(p, p, p, p); setTextIsSelectable(true)
        }
        MaterialAlertDialogBuilder(ctx).setTitle(getString(R.string.properties)).setView(tv)
            .setPositiveButton(getString(R.string.close), null)
            .setNeutralButton(getString(R.string.copy_path)) { _, _ -> copyPath(f) }.show()
    }

    // ── Rename ─────────────────────────────────────────────────────────────
    private fun showRenameDialog(f: File) {
        val ctx = requireContext()
        val il  = TextInputLayout(ctx, null, com.google.android.material.R.attr.textInputOutlinedStyle).apply {
            hint = getString(R.string.new_name); boxBackgroundMode = TextInputLayout.BOX_BACKGROUND_OUTLINE
            val p = (16 * resources.displayMetrics.density).toInt(); setPadding(p, p, p, 0)
        }
        val input = TextInputEditText(ctx).apply { setText(f.nameWithoutExtension); selectAll(); isSingleLine = true }
        il.addView(input)
        MaterialAlertDialogBuilder(ctx).setTitle(getString(R.string.rename)).setView(il)
            .setPositiveButton(getString(R.string.save)) { _, _ ->
                val newName = input.text.toString().trim()
                if (newName.isBlank()) { snack(getString(R.string.name_is_empty_short)); return@setPositiveButton }
                val ext     = if (f.isFile) ".${f.extension}" else ""
                val newFile = File(f.parent, "$newName$ext")
                if (f.renameTo(newFile)) { snack(getString(R.string.renamed_successfully)); refreshList() }
                else snack(getString(R.string.rename_failed))
            }.setNegativeButton(getString(R.string.cancel), null).show()
    }

    // ── Share / Copy path / Install APK ───────────────────────────────────
    private fun shareFile(f: File) {
        val uri = try { FileProvider.getUriForFile(requireContext(), "${requireContext().packageName}.provider", f) }
                  catch (_: Exception) { snack(getString(R.string.share_failed)); return }
        startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).apply {
            type = "*/*"; putExtra(Intent.EXTRA_STREAM, uri); addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }, getString(R.string.share_via)))
    }

    private fun copyPath(f: File) {
        val cm = requireContext().getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText("path", f.absolutePath)); snack(getString(R.string.path_copied))
    }

    private fun installApk(f: File) {
        val uri = try { FileProvider.getUriForFile(requireContext(), "${requireContext().packageName}.provider", f) }
                  catch (_: Exception) { snack(getString(R.string.cannot_install)); return }
        startActivity(Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        })
    }

    // ── Delete ─────────────────────────────────────────────────────────────
    private fun confirmDelete(f: File) {
        MaterialAlertDialogBuilder(requireContext()).setTitle(getString(R.string.delete))
            .setMessage(getString(R.string.delete_item_confirm, f.name) + if (f.isDirectory) "\n${getString(R.string.all_contents_will_be_deleted)}" else "")
            .setPositiveButton(getString(R.string.delete)) { _, _ ->
                val ok = if (f.isDirectory) f.deleteRecursively() else f.delete()
                if (ok) { snack(getString(R.string.deleted_successfully)); refreshList() } else snack(getString(R.string.delete_failed))
            }.setNegativeButton(getString(R.string.cancel), null).show()
    }

    // ── Storage detection ──────────────────────────────────────────────────
    private fun detectStorageRoots(): List<StorageRoot> {
        val roots    = mutableListOf<StorageRoot>()
        val internal = Environment.getExternalStorageDirectory()
        if (internal.canRead()) roots.add(StorageRoot(getString(R.string.internal_storage), internal))
        val ctx = requireContext()
        ctx.getExternalFilesDirs(null).forEachIndexed { i, f ->
            if (i == 0) return@forEachIndexed
            val sd = f?.parentFile?.parentFile?.parentFile?.parentFile
            if (sd != null && sd.canRead() && sd != internal) roots.add(StorageRoot(getString(R.string.sd_card), sd))
        }
        return roots.ifEmpty { listOf(StorageRoot(getString(R.string.storage), internal)) }
    }

    // ── Helpers ────────────────────────────────────────────────────────────
    private fun snack(msg: String) { view?.let { Snackbar.make(it, msg, Snackbar.LENGTH_SHORT).show() } }

    fun onBackPressed(): Boolean = navigateUp()

    override fun onDestroyView() {
        super.onDestroyView()
        // FIX: close the dialog first — dismiss() triggers setOnDismissListener
        // which releases mediaPlayer automatically, so we don't release it twice.
        audioDialog?.let { if (it.isShowing) it.dismiss() }
        audioDialog = null
        mediaPlayer?.apply { if (isPlaying) stop(); release() }; mediaPlayer = null
    }

    // ── Models ────────────────────────────────────────────────────────────
    data class StorageRoot(val label: String, val file: File)
    enum class SortMode { NAME_ASC, NAME_DESC, SIZE_ASC, SIZE_DESC, DATE_DESC, DATE_ASC }
}
