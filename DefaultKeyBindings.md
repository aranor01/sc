# Modifier keys representation

- A: Alt
- C: Ctrl
- S: Shift


# Default Key Bindings

- A-Up (Alt-Up)
       go to the parent directory of the active panel

- A-Left (Alt-Left)
       go back to the previous directory in the active panel's history

- A-Right (Alt-Right)
       go forward in the active panel's history

- Tab
       switch focus to the other panel

- F2
       open the user menu

- F10
       exit the application

- A-, (Alt-comma)
       toggle panel layout between vertical and horizontal

- A-i
       open the same directory shown in the active panel in the inactive panel

- Insert
       tag or untag the currently selected file and move to the next entry

- * (Asterisk)
       invert the tags in the current panel (tag all untagged entries and untag all tagged ones),
       if the command line is empty; otherwise appends `*` to the command line

- + (Plus)
       open the select group dialog: tags all visible entries in the active panel matching
       the given pattern (shell glob by default; enable **RegExp** in the dialog for regex matching);
       the dialog also offers **Files only** and **Case sensitive** options

- - (Minus)
       open the unselect group dialog: untags all visible entries in the active panel matching
       the given pattern (same options as `+`)

- F5
       copy the tagged files (or if there are no tagged files, the selected file) in the active panel to the directory shown by the inactive panel

- F6
       move the tagged files (or if there are no tagged files, the selected file) in the active panel to the directory shown by the inactive panel

- S-F6
       rename the currently selected file

- F7
       create a new directory in the active panel

- F8
       delete the tagged files (or if there are no tagged files, the selected file) in the active panel

- A-Enter
       copy the currently selected file name to the command line

- C-Enter
       same as A-Enter

- C-S-Enter
       copy the full path name of the currently selected file to the command line.

- A-Tab
       mimics bash autocompletion for the text currently typed on the command line: completes filenames, commands, variables, usernames and hostnames.

- C-x t, C-x C-t
       copy the tagged files (or if there are no tagged files, the selected file) of the active panel (C-x t) or of the inactive panel (C-x C-t) to the command line.

- C-x p, C-x C-p
       the first key sequence copies the active panel's path name to the command line, and the second one copies the inactive panel's path name to the command line.

- C-r
       open reverse-i-search: filters the command history by the current command line text.
       The prompt changes to `(reverse-i-search): `. All matching entries are shown above the
       command line (most recent highlighted). Typing or Backspace re-filters the list while
       keeping the current highlight if the entry is still present. Enter or Tab accepts the
       highlighted entry (replacing the whole command line); ESC closes the popup without
       changing the command line.

- A-r
       force-refresh the active panel's directory listing (re-reads the directory from disk)

- C-Up
       go to the previous command in the history, if any

- C-Down
       go to the next command in the history, if any

- C-s
       open the sort panel popup: choose sort order for the active panel (Name, Extension,
       Size, Modified, or Unsorted; ascending or descending)

- A-. (Alt-dot)
       toggle visibility of hidden files (dotfiles) in the active panel

- / (Slash)
       open quicksearch in the active panel: the cmdline area shows a `Search:` prompt and
       the cursor jumps to the first entry whose name starts with the typed string
       (case-insensitive prefix match); Enter or Esc closes the prompt

- A-s
       same as / (alternate quicksearch shortcut)

- C-f
       open the filter dialog for the active panel: hides entries not matching the given
       pattern (shell glob by default; enable **RegExp** in the dialog for regex matching);
       the dialog also offers **Files only** (directories always shown) and **Case sensitive** options;
       an empty pattern removes the filter; the filter persists across directory navigation
       within the session but is not saved to disk

- A-H
       open the path history popup for the active panel (most recent directory first)

- C-\
       open the bookmarks popup: navigate the active panel to a bookmarked directory.
       Del removes the currently selected entry from the list

- C-b
       add the active panel's current directory to bookmarks

- C-A-b
       toggle command line visibility

- A-b
       toggle button bar visibility

- C-o
       toggle the output overlay of the last executed command (stateless mode);
       or toggle between sc UI and full interactive subshell access (subshell mode).

- A-o (Alt-O)
       same as C-o, but in subshell mode also copies the sc command line into the
       subshell's readline buffer before entering passthrough.
       Both C-o and A-o exit the subshell back to the SC UI.
