  get_completions() {
      local cmd="$1" cur="$2"
      local COMP_LINE="$cmd $cur"
      local COMP_POINT=${#COMP_LINE}
      local COMP_WORDS=($COMP_LINE)
      local COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))

      _completion_loader "$cmd"

      local func=$(complete -p "$cmd" 2>/dev/null | grep -oP '(?<=-F )\S+')
      [ -z "$func" ] && return
    
      COMPREPLY=()
      "$func"
      printf '%s\n' "${COMPREPLY[@]}"
  }
