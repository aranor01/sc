# Modifier keys representation

- A: Alt
- C: Ctrl
- S: Shift


# Default Key Bindings

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

- F5
       copy the tagged files (or if there are no tagged files, the selected file) in the active panel to the directory shown by the inactive panel

- F6
       move the tagged files (or if there are no tagged files, the selected file) in the active panel to the directory shown by the inactive panel

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

- C-Up
       go to the previous command in the history, if any

- C-Down
       go to the next command in the history, if any

- C-A-b
       toggle command line visibility

- A-b
       toggle button bar visibility

- C-o
       toggle the output overlay of the last executed command (stateless mode);
       or toggle between sc UI and full interactive subshell access (subshell mode).
