package io.github.hafijbge.wordcraft;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.Parcelable;
import android.provider.OpenableColumns;
import android.util.Log;
import android.widget.Toast;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.FileReader;
import java.io.FileWriter;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayDeque;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * The Rust app (libwordcraft_android.so) in a NativeActivity, plus what only Java can reach:
 * Android's own file picker (the Storage Access Framework) and documents other apps open with us.
 *
 * <p>The editor works on files. A document picked in the system picker, or sent by a file manager
 * ("Open with"), is copied into the app's folder and the Rust side opens that copy; Save As and
 * Export pick a destination and get a path in the app's folder to write. Each copy remembers its
 * document (a content URI) in {@code links.tsv}, and {@link #syncBack()}, which the Rust side calls
 * every few seconds, copies a file that changed back to its document.
 *
 * <p>The Rust side calls the public methods on its own thread (JNI); they never block it.
 */
public class WriterActivity extends NativeActivity {
    private static final String TAG = "WorkSuiteWriter";
    private static final int PICK_OPEN = 1;
    private static final int PICK_SAVE = 2;

    private static final Object LOCK = new Object();
    /** The picker's answer: null while there is none, "" when cancelled, else a path. */
    private static String result;
    /** Paths of documents other apps opened with us, oldest first. */
    private static final ArrayDeque<String> opened = new ArrayDeque<>();
    /** Local copy → its document and the copy's last-modified time when it last matched. */
    private static final Map<String, Link> links = new LinkedHashMap<>();
    private static boolean linksLoaded;
    private static volatile boolean syncing;
    /** The name Save As or Export suggested, until its picker answers. */
    private static String pendingName = "document";

    private static final class Link {
        final String uri;
        long synced;

        Link(String uri, long synced) {
            this.uri = uri;
            this.synced = synced;
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        loadLinks();
        handleIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    @Override
    protected void onPause() {
        syncBack();
        super.onPause();
    }

    // ---- Called from Rust ----

    /** Show the system picker to open a file; {@code purpose} is "document", "picture"… */
    public void pickOpen(String purpose) {
        synchronized (LOCK) {
            result = null;
        }
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        if ("picture".equals(purpose)) {
            intent.setType("image/*");
        } else if ("document".equals(purpose)) {
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
                "application/vnd.ms-word.document.macroEnabled.12",
                "application/msword",
                "application/vnd.oasis.opendocument.text",
                "application/rtf",
                "text/rtf",
                "text/plain",
                "text/markdown",
                "text/html",
                "text/xml",
                "application/json",
                // Some storage apps report every .docx as plain bytes.
                "application/octet-stream",
            });
        } else {
            intent.setType("*/*");
        }
        start(intent, PICK_OPEN);
    }

    /** Show the system picker to choose where to save a file called {@code name}. */
    public void pickSave(String name) {
        synchronized (LOCK) {
            result = null;
            pendingName = safeName(name);
        }
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(mimeFor(name));
        intent.putExtra(Intent.EXTRA_TITLE, name);
        start(intent, PICK_SAVE);
    }

    /** The picker's answer once, then null: "" when cancelled, else the path to use. */
    public String takeResult() {
        synchronized (LOCK) {
            String r = result;
            result = null;
            return r;
        }
    }

    /** The next document another app opened with us (a path), or null. */
    public String takeOpened() {
        synchronized (LOCK) {
            return opened.poll();
        }
    }

    /** Copy every local copy that changed since the last time back to its document. */
    public void syncBack() {
        if (syncing) {
            return;
        }
        syncing = true;
        new Thread(() -> {
            try {
                Map<String, Link> snapshot;
                synchronized (LOCK) {
                    snapshot = new LinkedHashMap<>(links);
                }
                boolean changed = false;
                for (Map.Entry<String, Link> e : snapshot.entrySet()) {
                    File file = new File(e.getKey());
                    long modified = file.exists() ? file.lastModified() : 0;
                    if (modified == 0 || modified == e.getValue().synced) {
                        continue;
                    }
                    try (InputStream in = new FileInputStream(file); OutputStream out = openForWrite(Uri.parse(e.getValue().uri))) {
                        copy(in, out);
                        synchronized (LOCK) {
                            e.getValue().synced = modified;
                        }
                        changed = true;
                    } catch (IOException | RuntimeException ex) {
                        Log.w(TAG, "couldn't write back " + e.getKey(), ex);
                        toast("Couldn't save to the original file. Use Save As to choose another place.");
                        synchronized (LOCK) {
                            // Don't try again until the next change.
                            e.getValue().synced = modified;
                        }
                    }
                }
                if (changed) {
                    saveLinks();
                }
            } finally {
                syncing = false;
            }
        }).start();
    }

    // ---- The picker's answer, and other apps' documents ----

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != PICK_OPEN && requestCode != PICK_SAVE) {
            return;
        }
        Uri uri = resultCode == RESULT_OK && data != null ? data.getData() : null;
        if (uri == null) {
            answer("");
            return;
        }
        keepPermission(uri, data.getFlags());
        if (requestCode == PICK_OPEN) {
            new Thread(() -> {
                String path = importDocument(uri);
                answer(path == null ? "" : path);
            }).start();
        } else {
            String name;
            synchronized (LOCK) {
                name = pendingName;
            }
            String shown = displayName(uri);
            File file = new File(newFolder(), safeName(shown != null ? shown : name));
            link(file, uri, 0);
            answer(file.getAbsolutePath());
        }
    }

    private void handleIntent(Intent intent) {
        if (intent == null) {
            return;
        }
        Uri uri = null;
        String action = intent.getAction();
        if (Intent.ACTION_VIEW.equals(action) || Intent.ACTION_EDIT.equals(action)) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(action)) {
            Parcelable stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (stream instanceof Uri) {
                uri = (Uri) stream;
            }
        }
        if (uri == null) {
            return;
        }
        // Handled once, also when the activity is recreated with the same intent.
        intent.setAction(Intent.ACTION_MAIN);
        intent.setData(null);
        final Uri document = uri;
        new Thread(() -> {
            String path = importDocument(document);
            if (path != null) {
                synchronized (LOCK) {
                    opened.add(path);
                }
            }
        }).start();
    }

    /** Copy a document into the app's folder and remember where it came from; null on failure. */
    private String importDocument(Uri uri) {
        String name = displayName(uri);
        File file = new File(newFolder(), safeName(name != null ? name : "document.docx"));
        try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(file)) {
            if (in == null) {
                throw new IOException("no stream");
            }
            copy(in, out);
        } catch (IOException | RuntimeException e) {
            Log.w(TAG, "couldn't read " + uri, e);
            toast("Couldn't open that file.");
            return null;
        }
        link(file, uri, file.lastModified());
        return file.getAbsolutePath();
    }

    // ---- Helpers ----

    private void start(Intent intent, int request) {
        runOnUiThread(() -> {
            try {
                startActivityForResult(intent, request);
            } catch (RuntimeException e) {
                Log.w(TAG, "no file picker", e);
                toast("This device has no file picker.");
                answer("");
            }
        });
    }

    private static void answer(String r) {
        synchronized (LOCK) {
            result = r;
        }
    }

    private void keepPermission(Uri uri, int flags) {
        int keep = flags & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        try {
            getContentResolver().takePersistableUriPermission(uri, keep);
        } catch (RuntimeException e) {
            // Not every provider offers lasting access: the copy still opens, and saves back while
            // the app runs.
            Log.i(TAG, "no lasting access to " + uri);
        }
    }

    private OutputStream openForWrite(Uri uri) throws IOException {
        OutputStream out;
        try {
            // Truncate: a shorter document mustn't keep the old one's tail.
            out = getContentResolver().openOutputStream(uri, "wt");
        } catch (IllegalArgumentException | UnsupportedOperationException e) {
            out = getContentResolver().openOutputStream(uri, "w");
        }
        if (out == null) {
            throw new IOException("no stream");
        }
        return out;
    }

    private String displayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                return c.getString(0);
            }
        } catch (RuntimeException e) {
            Log.i(TAG, "no name for " + uri);
        }
        String last = uri.getLastPathSegment();
        return last != null && last.contains(".") ? last.substring(last.lastIndexOf('/') + 1) : null;
    }

    /** A new folder for one document, so two files with the same name don't collide. */
    private File newFolder() {
        File dir = new File(new File(getFilesDir(), "documents"), Long.toString(System.nanoTime()));
        if (!dir.mkdirs() && !dir.isDirectory()) {
            Log.w(TAG, "couldn't make " + dir);
        }
        return dir;
    }

    private void link(File file, Uri uri, long synced) {
        synchronized (LOCK) {
            links.put(file.getAbsolutePath(), new Link(uri.toString(), synced));
        }
        saveLinks();
    }

    private File linksFile() {
        return new File(getFilesDir(), "links.tsv");
    }

    private void loadLinks() {
        synchronized (LOCK) {
            if (linksLoaded) {
                return;
            }
            linksLoaded = true;
            File f = linksFile();
            if (!f.exists()) {
                return;
            }
            try (BufferedReader r = new BufferedReader(new FileReader(f))) {
                String line;
                while ((line = r.readLine()) != null) {
                    String[] p = line.split("\t");
                    if (p.length == 3 && new File(p[0]).exists()) {
                        try {
                            links.put(p[0], new Link(p[1], Long.parseLong(p[2])));
                        } catch (NumberFormatException e) {
                            // A damaged line: skip it.
                        }
                    }
                }
            } catch (IOException e) {
                Log.w(TAG, "couldn't read links", e);
            }
        }
    }

    private void saveLinks() {
        StringBuilder b = new StringBuilder();
        synchronized (LOCK) {
            for (Map.Entry<String, Link> e : links.entrySet()) {
                b.append(e.getKey()).append('\t').append(e.getValue().uri).append('\t').append(e.getValue().synced).append('\n');
            }
        }
        File f = linksFile();
        File tmp = new File(f.getPath() + ".tmp");
        try (FileWriter w = new FileWriter(tmp)) {
            w.write(b.toString());
        } catch (IOException e) {
            Log.w(TAG, "couldn't save links", e);
            return;
        }
        if (!tmp.renameTo(f)) {
            Log.w(TAG, "couldn't replace links");
        }
    }

    private void toast(String message) {
        runOnUiThread(() -> Toast.makeText(this, message, Toast.LENGTH_LONG).show());
    }

    private static void copy(InputStream in, OutputStream out) throws IOException {
        byte[] buf = new byte[64 * 1024];
        int n;
        while ((n = in.read(buf)) > 0) {
            out.write(buf, 0, n);
        }
        out.flush();
    }

    /** A file name without path separators or control characters. */
    private static String safeName(String name) {
        String s = name == null ? "" : name.replaceAll("[/\\\\\\p{Cntrl}]", "_").trim();
        if (s.isEmpty() || s.equals(".") || s.equals("..")) {
            return "document";
        }
        return s.length() > 120 ? s.substring(s.length() - 120) : s;
    }

    private static String mimeFor(String name) {
        String n = name == null ? "" : name.toLowerCase();
        String ext = n.contains(".") ? n.substring(n.lastIndexOf('.') + 1) : "";
        switch (ext) {
            case "docx": return "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
            case "dotx": return "application/vnd.openxmlformats-officedocument.wordprocessingml.template";
            case "docm": return "application/vnd.ms-word.document.macroEnabled.12";
            case "doc": return "application/msword";
            case "odt": return "application/vnd.oasis.opendocument.text";
            case "rtf": return "application/rtf";
            case "pdf": return "application/pdf";
            case "txt": return "text/plain";
            case "md": return "text/markdown";
            case "html": case "htm": return "text/html";
            case "tex": return "application/x-tex";
            case "epub": return "application/epub+zip";
            case "png": return "image/png";
            case "jpg": case "jpeg": return "image/jpeg";
            case "svg": return "image/svg+xml";
            case "xml": return "text/xml";
            case "json": return "application/json";
            default: return "application/octet-stream";
        }
    }
}
