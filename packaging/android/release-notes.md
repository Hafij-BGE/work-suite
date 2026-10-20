WorkSuite, made by Md Hafijur Rahman: a free, open-source office suite for Android, with
**Writer**, **Slides** and **Sheets** in one app.

**Preview:** expect rough edges, and please report what you find in an issue.

## New in 0.9.6: import a .zip of fonts, and no font notice for Symbol and Wingdings

- Documents with bullets or ticks in Word's symbol fonts (Symbol, Wingdings, Wingdings 2 and 3,
  Webdings) no longer show the red "not available" bar: Writer and Slides draw those characters
  as the symbols they stand for, so nothing is missing. An installed symbol font is still used.
- The font bar offers Google Fonts only for fonts Google Fonts has.
- **Import fonts…** (on the home screen, and in the font bar's card): a font file, or a .zip of
  fonts copied from a computer, installed in Writer, Slides and Sheets in one go.

## New in 0.9.5: missing fonts, fixed in two taps

- **A red bar says when a font is missing.** When a document, presentation or workbook uses a
  font this device doesn't have, a bar over the app names it ("“Aptos” is not available on this
  device") and the font shown in its place. Tap **Download font** or **Choose font**:
  - **Download from Microsoft:** Aptos, and Microsoft's free Core fonts for the Web (Arial, Times
    New Roman, Courier New, Georgia, Verdana…), straight from Microsoft, for this device only.
  - **Download from Google Fonts:** any font Google Fonts has. For Calibri and Cambria, which have no
    free download, Carlito and Caladea: the same widths, so lines and pages break as in Word.
  - **Add a font file…:** a .ttf or .otf file on this device.
  - **Choose a similar font:** an installed font in its place, throughout the document (Undo takes
    it back).

  A font installed in one app works in all three, and stays after a restart. "Don't show this
  again" hides the bar; the home screen turns it back on.
- **Documents open for reading.** A file opened from the home screen or with Open with shows in
  Read Mode: one column that fits the screen, with page buttons. Tap **Edit Document** to edit.
- **The status bar fits a phone:** the page, word count and font notice no longer run into each
  other or into the zoom controls.
- WorkSuite now uses the internet, only for a font download you ask for. Your documents never
  leave the device.

## Updating from 0.9.3: uninstall it first

0.9.4 is signed with a new release key, because 0.9.3's key was lost. Android won't install it over
0.9.3, so save your documents, uninstall WorkSuite 0.9.3, then install 0.9.4. Your documents stay
where you saved them; settings and recent files start fresh. Versions after 0.9.4 install as
updates again.

## New in 0.9.4: missing fonts are named, and can be installed

- **Writer says when a font isn't installed.** A document set in a font this device doesn't have
  (Aptos, Calibri…) is drawn with a similar font, whose widths differ, so lines and pages can break
  differently from Word. Writer now says which font is missing and what it is shown with, in the
  status bar and when the file opens; Slides and Sheets say so too.
- **Install fonts on the device**: Add Font… copies font files into the app's font folder, so text
  set in them looks as intended from then on.
- Comparison tools for developers: a set of test documents and a page-by-page comparison with
  Word's own PDF export (docs/word-fidelity.md).

## New in 0.9.3: one key for every release, and only WorkSuite's name

- WorkSuite is now signed with a permanent key, under its own app id
  (`io.github.hafijbge.worksuite`): 0.9.3 installs **next to** an older WorkSuite instead of
  failing with a package conflict. Open your documents in the old one and save them, then uninstall
  it.
- The apps show only WorkSuite's name and icon: About shows the WorkSuite icon and its maker, and
  the apps call themselves Writer, Slides and Sheets throughout.

## New in 0.9.2: stylus support

- **A pen works like a pen.** With a stylus (S Pen, USI or another active pen) the tip selects
  text, picks cells and moves objects precisely, while fingers keep scrolling and zooming. The pen
  hovers (buttons light up under it), and its side button opens the menu a right-click opens.
- **Draw with it:** the **Draw** tab is back in Writer, Slides and Sheets: pick a pen, pencil or
  highlighter and write or sketch on the page; fingers still scroll while a pen is picked.
- **The eraser end erases** (on pens that have one), whatever tool is picked; the tool comes back
  when you lift it.
- **Palm rejection:** a hand resting on the screen while the pen is on or near it is ignored.
- A mouse (or a keyboard cover's trackpad) now works as a mouse too: it hovers, and its right
  button opens the menu.

## New in 0.9.1

- **A file you open shows at once.** Opening from the home screen (or "Open with") no longer leaves
  Writer on its start page, and a document opened again keeps its one copy (no duplicates under
  Recent, which now says "From your files" instead of the app's internal folder).
- **Back works everywhere:** it closes a menu, dialog or the File view first, then returns to the
  WorkSuite home screen; on the home screen it puts the app in the background.
- **No more overlap with the status bar:** the app draws edge to edge and keeps its controls clear
  of the status bar, navigation bar, camera cut-out and keyboard; the bars' icons follow the app's
  light or dark look.
- **Start pages fit the screen:** template and theme tiles in Writer, Slides and Sheets wrap on
  phones and tablets instead of running off the right edge.
- **A redesigned home screen and app switcher.**
- **Fully offline:** the app has no network permission, the file picker shows only files on this
  device, and the online community link is gone from the Slides start page.
- Made by Md Hafijur Rahman.

## New in 0.9.0

- **One suite, three apps:** a home screen to start Writer (documents), Slides (presentations) or
  Sheets (spreadsheets), and a bar at the top to switch between them; each keeps its files open.
- **Open a file…** on the home screen, and "Open with" from file managers, open each file in the
  app it belongs to (.docx and text in Writer, .pptx in Slides, .xlsx and .csv in Sheets).
- **Slides and Sheets for phones:** the same lighter ribbon and short File menu as Writer (Home,
  New, Open, Save, Save As, Export, Close, About), touch scrolling, pinch zoom and Android's file
  picker.
- New name and icon: *WorkSuite*.

## Install

1. Download `WorkSuite-0.9.3-android-release.apk` below, on your phone.
2. Open it, and allow installing from that app when Android asks.

A WorkSuite older than 0.9.3 stays installed beside the new one (it has another app id): save
your documents from it, then uninstall it. From 0.9.3 on, new versions install over the old one.

For 64-bit ARM phones and tablets running Android 8 or later.

## Known limits

- The on-screen keyboard types letters, digits and punctuation; Bengali and other composed scripts,
  swipe typing and autocorrect may not work yet. A hardware keyboard works.
- Saving a new presentation or workbook asks where to save it; once saved, Save writes back to it.
- Copy and paste work inside the app only, not with other apps.
- No printing: export a PDF and print that.
- Pen strokes have the width you pick; they don't get thicker when you press harder. A passive
  (capacitive, rubber-tipped) stylus is a finger to Android, so it scrolls like one.
- On a phone, Writer's pens sit in the Draw tab's *Drawing Tools* menu (a tablet shows them all).
