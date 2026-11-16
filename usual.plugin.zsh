# Inside an anonymous function, so `return` can't end the .zshrc that evals this.
() {
[[ -o interactive && -o zle && $TERM != dumb ]] || return 0
whence -p usual >/dev/null || return 0   # not $commands[usual]: that hashes all of $PATH
[[ $ZSH_VERSION == (5.<3->*|<6->*) ]] || return 0   # 5.3+: line-pre-redraw hook

typeset -g _usual_show=always _usual_list=1 _usual_run=0
typeset -g _usual_history=${XDG_STATE_HOME:-$HOME/.local/state}/usual/history
typeset -g _usual_hints=${XDG_CACHE_HOME:-$HOME/.cache}/usual/hints
typeset -ga _usual_cmds _usual_descs
typeset -g _usual_pwd= _usual_root= _usual_i=1 _usual_on=0 _usual_text= _usual_buf= _usual_pw=
typeset -ga _usual_hls
typeset -g _usual_ell=… _usual_utf8=1 _usual_pmax=-1
typeset -gA _usual_orig _usual_ours
typeset -gA _usual_names=(accept tab next ↓ previous ↑)
typeset -g _usual_typing=0 _usual_typed= _usual_sel=0 _usual_base=0
typeset -ga _usual_matches
typeset -gA _usual_descof
typeset -gA _usual_starts
typeset -g _usual_memo= _usual_italic=
[[ $ZSH_VERSION == (5.<9->*|<6->*) ]] && _usual_memo=' memo=usual'
# zsh < 5.10 drops a highlight's memo after an unknown word like italic, leaking the hint's colour.
[[ $ZSH_VERSION == (5.<10->*|<6->*) ]] && _usual_italic=,italic
zmodload -F zsh/datetime p:EPOCHSECONDS 2>/dev/null
zmodload -F zsh/stat b:zstat 2>/dev/null
zmodload -F zsh/files b:zf_rm 2>/dev/null

_usual_settings() {
  case ${(L)USUAL_HINT} in
    (enter|once) _usual_show=once ;;
    (off|no|false|0|never) _usual_show=off ;;
    (*) _usual_show=always ;;
  esac
  [[ ${(L)USUAL_TYPING} == (off|no|false|0) ]] && _usual_list=0 || _usual_list=1
  [[ ${(L)USUAL_TAB} == run ]] && _usual_run=1 || _usual_run=0
}

_usual_style() {
  [[ $1 == <-> ]] && REPLY=fg=$1 || REPLY=$1
}

_usual_precmd() {
  emulate -L zsh
  _usual_settings
  # Still drawn: Ctrl-C under a prompt with its own INT trap (powerlevel10k) skipped our cleanup.
  _usual_pw=
  (( ${+functions[_zsh_autosuggest_modify]} )) && [[ $functions[_zsh_autosuggest_modify] != *_usual_orig_autosuggest_modify* ]] && _usual_shield
  _usual_typing=0 _usual_typed= _usual_sel=0
  if [[ -n $_usual_text ]]; then
    _usual_text= _usual_hls=()
    [[ $_usual_show == once ]] && _usual_on=0
  fi
  [[ $PWD == $_usual_pwd && $_usual_show != always ]] && return
  _usual_pwd=$PWD
  local out root
  local -a reply
  if ! _usual_saved; then
    out=$(command usual suggest "$PWD" 2>/dev/null) || out=
    reply=("${(@f)out}")
  fi
  local n=${reply[2]:-0}
  [[ $n == <-> ]] || n=0
  root=$reply[1]
  _usual_cmds=("${(@)reply[3,2+n]}") _usual_descs=("${(@)reply[3+n,2+2*n]}")
  _usual_starts=() _usual_descof=()
  (( n )) && _usual_descof=("${(@)_usual_cmds:^_usual_descs}")
  if [[ $root != $_usual_root || $_usual_show == always ]]; then
    _usual_root=$root _usual_i=1 _usual_on=1
    [[ $_usual_show == off ]] && _usual_on=0
  fi
  (( _usual_i <= $#_usual_cmds )) || _usual_i=1
}

_usual_saved() {
  local file=$_usual_hints$PWD/.hint i n
  [[ -r $file ]] || return 1
  local -a l=("${(@f)$(<$file)}") st
  [[ $l[1] == 'usual-hints 2' && $l[3] == $PATH ]] && (( ${EPOCHSECONDS:-0} < l[2] )) || return 1
  n=$l[4]
  for (( i = 5; i < 5 + 2 * n; i += 2 )); do
    zstat -H st -- $l[i] 2>/dev/null && [[ $st[mtime].$st[size] == "$l[i+1]" ]] || return 1
  done
  reply=("${(@)l[5+2*n,-1]}")
}

_usual_preexec() {
  emulate -L zsh
  [[ -n $_usual_root && -n $_usual_history && -n $EPOCHSECONDS ]] || return
  local line=${${1//[$'\t\n']/ }## #} c
  for c in $_usual_cmds; do
    if [[ $line == "$c" || $line == "$c "* ]]; then
      print -r -- "$EPOCHSECONDS"$'\t'"$_usual_root"$'\t'"$c" >>| $_usual_history 2>/dev/null
      return
    fi
  done
}

# zsh-autosuggestions shares the space after the cursor: hide the hint from it, keep its colour off.
_usual_shield() {
  local f
  for f in modify accept partial_accept execute suggest highlight_apply; do
    (( ${+functions[_zsh_autosuggest_$f]} )) || return
    functions[_usual_orig_autosuggest_$f]=$functions[_zsh_autosuggest_$f]
  done
  for f in modify accept partial_accept execute; do
    functions[_zsh_autosuggest_$f]="[[ -n \$_usual_text && \$POSTDISPLAY == \"\$_usual_text\" ]] && POSTDISPLAY=
_usual_orig_autosuggest_$f \"\$@\""
  done
  _zsh_autosuggest_suggest() {
    _usual_orig_autosuggest_suggest "$@"
    [[ -n $POSTDISPLAY ]] || _usual_draw
  }
  _zsh_autosuggest_highlight_apply() {
    [[ -n $_usual_text && $POSTDISPLAY == "$_usual_text" ]] || _usual_orig_autosuggest_highlight_apply "$@"
  }
}

# zsh 5.9+ finds our highlights by tag; earlier, by position, which misses some.
_usual_clear() {
  [[ -n $_usual_text && $POSTDISPLAY == $_usual_text ]] && POSTDISPLAY=
  if [[ -n $_usual_memo ]]; then
    region_highlight=("${(@)region_highlight:#*$_usual_memo}")
  elif (( $#_usual_hls )); then
    region_highlight=("${(@)region_highlight:|_usual_hls}")
  fi
  _usual_text= _usual_hls=()
}

_usual_put() {
  [[ -n $2 ]] && _usual_hls+=("$(( _usual_base + $#_usual_buf )) $(( _usual_base + $#_usual_buf + $#1 )) $2$_usual_memo")
  _usual_buf+=$1
}

# Without a UTF-8 locale zsh counts bytes, not columns.
_usual_promptwidth() {
  local subst=0 p=$PS1
  [[ -o promptsubst ]] && subst=1
  emulate -L zsh -o extendedglob
  reply=(-1 -1)
  [[ $p == *('$('|'`')* ]] && return
  (( subst )) && p=${(e)p}
  p=${${(%)p}##*$'\n'}
  p=${p//$'\e'\[[0-9;?]#[@-~]/}                          # colours and other CSI sequences
  p=${p//$'\e'\][^$'\a\e']#($'\a'|$'\e\\')/}           # terminal titles, shell-integration marks
  if (( ! _usual_utf8 )) && [[ $p == *[^\ -~]* ]]; then
    (( $#p < COLUMNS )) && reply=(-1 $#p) || reply=(-1 $(( COLUMNS - 14 )))   # wraps: leave the least room
    return
  fi
  reply=($(( ${(m)#p} % COLUMNS )) $(( ${(m)#p} % COLUMNS )))
}

_usual_fit() {
  REPLY=$1
  (( ${(m)#REPLY} <= $2 )) && return
  (( $2 < 2 )) && { REPLY=; return }
  REPLY=${REPLY[1,$2-1]}
  while (( ${(m)#REPLY} > $2 - 1 )); do REPLY=${REPLY[1,-2]}; done
  REPLY=${REPLY%% #}$_usual_ell
}

_usual_fitcmd() {
  # Too long: drop the runner in front, since the script name is what you read.
  local s=$1 ws=(${=1})
  if (( ${(m)#s} > $2 && $#ws <= 3 )) && [[ $s != *' -'* ]]; then
    while (( $#ws > 1 && ${(m)#${(j: :)ws}} + 1 > $2 )); do shift ws; done
    s=${(j: :)ws}
    (( ${(m)#s} + 1 <= $2 )) && { REPLY=$_usual_ell$s; return }
  fi
  _usual_fit $s $2
}

_usual_fitdesc() {
  # Too long: keep the clauses that fit, or nothing.
  local cut='(, |: |; |. | \(| — | - )'
  REPLY=$1
  (( ${(m)#REPLY} > $2 )) && [[ $REPLY == ?*': '* ]] && REPLY=${REPLY%%: *}
  while (( ${(m)#REPLY} > $2 )); do
    [[ $REPLY == *${~cut}* ]] || { REPLY=; return }
    REPLY=${REPLY%${~cut}*}
  done
}

_usual_draw() {
  # Keys still pending (fast typing, paste): change nothing, the next key needs this state.
  (( PENDING )) && return 0
  [[ -n $BUFFER ]] && _usual_i=1
  if [[ -z $_usual_text ]]; then
    if [[ -z $BUFFER ]]; then
      (( _usual_on )) || return 0
    else
      [[ $_usual_show == once ]] && _usual_on=0
      (( $#_usual_cmds && _usual_list )) || return 0
    fi
  fi
  emulate -L zsh -o extendedglob
  [[ -n $BUFFER && $_usual_show == once ]] && _usual_on=0
  [[ -n $_usual_text && $LASTWIDGET == vi-cmd-mode ]] && _usual_on=0 _usual_typing=0
  _usual_clear
  (( $#_usual_cmds && COLUMNS >= 40 )) && [[ -z $PREDISPLAY && -z $POSTDISPLAY ]] || return 0

  # A recalled or completed line gets no list: ↓ there is history's.
  case $LASTWIDGET in
    (self-insert|magic-space|(vi-|)backward-delete-char|(vi-|)backward-kill-word|usual-next|usual-previous) _usual_typing=1 ;;
    (autosuggest-*|*reset-prompt*|*redisplay*|zle-*|_p9k_*|_zsh_highlight*|*async*|'') ;;
    (*) _usual_typing=0 ;;
  esac

  local i n w=0 k j d room width=$(( COLUMNS - 1 )) pw indent line1
  local -a shown
  if [[ -z $BUFFER ]]; then
    (( _usual_on )) || return 0
    n=$#_usual_cmds i=$_usual_i
  else
    (( _usual_typing && CURSOR == $#BUFFER && $#BUFFER <= 40 )) &&
      (( _usual_list )) && [[ $BUFFER == [A-Za-z0-9./]([A-Za-z0-9./@:_+=\ -])# ]] || return 0
    _usual_match
    n=$#_usual_matches i=$_usual_sel
    (( n )) || return 0
  fi

  _usual_utf8=0 _usual_ell='~'
  [[ ${LC_ALL:-${LC_CTYPE:-$LANG}} == *(#i)utf(-|)8* ]] && _usual_utf8=1 _usual_ell='…'
  [[ -n $_usual_pw ]] || { _usual_promptwidth; _usual_pw=$reply[1] _usual_pmax=$reply[2] }
  pw=$_usual_pw

  local rows=${USUAL_ROWS:-4}
  [[ $rows == <1-> ]] || rows=4
  (( rows > (LINES - 2) / 3 )) && rows=$(( (LINES - 2) / 3 ))
  (( rows < 1 )) && rows=1

  if (( pw >= 0 && pw <= COLUMNS / 2 )); then
    indent=${(l:pw:)} line1=$(( width - pw ))
  elif (( _usual_pmax >= 0 )); then
    indent='  ' line1=$(( width - _usual_pmax ))
  else
    indent='  ' line1=$(( width / 2 ))
  fi

  local sc sd sk
  if [[ $TERM == *256col* || -n $COLORTERM ]]; then
    sc=fg=244 sd=fg=240 sk=fg=246
    [[ ${COLORFGBG##*;} == (7|15) ]] && sc=fg=244 sd=fg=249 sk=fg=242
  else
    sc=fg=8 sd=fg=8 sk=fg=8
  fi
  [[ -n $ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE ]] && sc=$ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE
  [[ -n $USUAL_COMMAND_STYLE ]] && { _usual_style $USUAL_COMMAND_STYLE; sc=$REPLY }
  [[ -n $USUAL_DESCRIPTION_STYLE ]] && { _usual_style $USUAL_DESCRIPTION_STYLE; sd=$REPLY }
  [[ -n $USUAL_KEY_STYLE ]] && { _usual_style $USUAL_KEY_STYLE; sk=$REPLY }

  local rule='╰─ ' dot='  ·  ' mark='› ' ka=$_usual_names[accept] kn=$_usual_names[next] nav=$_usual_names[next]
  if (( i > 1 )) || [[ -n $BUFFER && $i -gt 0 ]]; then
    nav=$_usual_names[previous]$kn
    (( $#nav > 2 )) && nav=$_usual_names[previous]/$kn
  fi
  if (( ! _usual_utf8 )); then
    rule='`- ' dot='  -  ' mark='> '
    nav=${${${nav//↑↓/up\/dn}//↑/up}//↓/dn} kn=${kn//↓/dn} ka=${${${ka//→/right}//↑/up}//↓/dn}
  fi
  local -a parts
  _usual_buf= _usual_hls=() _usual_base=$#BUFFER

  if [[ -z $BUFFER ]]; then
    shown=($i)
    for (( k = 1; k < n && k < rows; k++ )); do shown+=($(( (i + k - 1) % n + 1 ))); done
    local w1
    for j in $shown; do (( $#_usual_cmds[j] > w )) && w=$#_usual_cmds[j]; done
    (( w > 28 )) && w=28
    (( w > (width - $#indent) / 2 )) && w=$(( (width - $#indent) / 2 ))
    w1=$(( w < line1 ? w : line1 ))
    (( w1 < 4 )) && return 0
    local cap=$ka
    (( _usual_run )) && cap="$ka to run"
    (( line1 - w1 >= $#cap + 2 )) || cap=
    _usual_fitcmd $_usual_cmds[i] $w1
    _usual_put "${(r:w1:)REPLY}" $sc$_usual_italic
    room=$(( line1 - w1 - 2 - (${#cap} ? $#cap + 2 : 0) ))
    d=$_usual_descs[i]
    (( _usual_utf8 )) || d=${d//[^\ -~]/}
    if (( room >= 8 )) && [[ -n $d ]]; then
      _usual_fitdesc $d $room
      [[ -n $REPLY ]] && _usual_put "  $REPLY" $sd
    fi
    [[ -n $cap ]] && { _usual_put '  '; _usual_put $cap $sk }
    for j in $shown[2,-1]; do _usual_row "$_usual_cmds[j]" "$_usual_descs[j]" $sc; done
    local count="${(l:$#n:)i}/$n"   # fixed width, so stepping doesn't shift it
    if [[ -n $cap ]]; then
      if (( n == 1 )); then
        parts=()
      elif (( $#indent + $#rule + ${#:-"$nav more$dot$count"} <= width )); then
        parts=($nav " more$dot$count")
      else
        parts=('' $count)
      fi
    elif (( n == 1 )); then
      parts=($ka ' accept')
    elif (( $#indent + $#rule + ${#:-"$ka accept  $nav more$dot$count"} <= width )); then
      parts=($ka ' accept  ' $nav " more$dot$count")
    elif (( $#indent + $#rule + ${#:-"$ka  $nav  $count"} <= width )); then
      parts=($ka '  ' $nav "  $count")
    else
      parts=('' $count)
    fi
  else
    (( rows > 1 )) && (( rows-- ))
    local from=1
    (( i > rows )) && from=$(( i - rows + 1 ))
    shown=("${(@)_usual_matches[from,from+rows-1]}")
    for j in $shown; do (( $#j > w )) && w=$#j; done
    (( w > 28 )) && w=28
    (( w > (width - $#indent) / 2 )) && w=$(( (width - $#indent) / 2 ))
    (( w < 4 )) && return 0
    for j in $shown; do
      if (( i )) && [[ $j == $_usual_matches[i] ]]; then
        _usual_row "$j" "$_usual_descof[$j]" bold ${indent[1,-1-$#mark]}$mark
      else
        _usual_row "$j" "$_usual_descof[$j]" $sc
      fi
    done
    local count="$n matches"
    (( n == 1 )) && count='1 match'
    (( i )) && count="$i/$n"
    if (( i )) && (( $#indent + $#rule + ${#:-"$ka accept  $nav$dot$count"} <= width )); then
      parts=($ka ' accept  ' $nav "$dot$count")
    elif (( ! i )) && (( $#indent + $#rule + ${#:-"$kn choose$dot$count"} <= width )); then
      parts=($kn " choose$dot$count")
    else
      parts=('' $count)
    fi
    rows=2
  fi

  if (( rows > 1 && $#parts )); then
    _usual_put $'\n'$indent
    _usual_put $rule $sd
    for (( k = 1; k <= $#parts; k++ )); do
      (( k % 2 )) && _usual_put "$parts[k]" bold || _usual_put "$parts[k]" $sd
    done
  fi

  POSTDISPLAY=$_usual_buf
  _usual_text=$_usual_buf
  region_highlight+=("${(@)_usual_hls}")
}

_usual_row() {
  _usual_put $'\n'${4-$indent}
  _usual_fitcmd $1 $w
  _usual_put "${(r:w:)REPLY}" $3
  room=$(( width - $#indent - w - 2 ))
  d=$2
  (( _usual_utf8 )) || d=${d//[^\ -~]/}
  if (( room >= 8 )) && [[ -n $d ]]; then
    _usual_fitdesc $d $room
    [[ -n $REPLY ]] && _usual_put "  $REPLY" $sd
  fi
}

_usual_match() {
  [[ $BUFFER == $_usual_typed ]] && return
  _usual_typed=$BUFFER _usual_sel=0 _usual_matches=()
  if (( ! $#_usual_starts )); then
    local c w
    for c in $_usual_cmds; do
      _usual_starts[$c[1]]=1
      for w in ${=c//[:_.\/-]/ }; do _usual_starts[$w[1,2]]=1; done
    done
  fi
  (( ${+_usual_starts[$BUFFER[1,2]]} )) || return
  local -a m1 m2
  m1=(${(M)_usual_cmds:#$BUFFER?*})
  (( $#BUFFER >= 2 )) && [[ $BUFFER != *' '* ]] && m2=(${(M)_usual_cmds:#*[\ :_./-]$BUFFER*}) && m2=(${m2:|m1})
  _usual_matches=($m1 $m2)
}

_usual_finish() {
  emulate -L zsh
  [[ -n $_usual_text ]] && { _usual_clear; zle -R 2>/dev/null; }
  [[ $_usual_show == once ]] && _usual_on=0
  _usual_typing=0 _usual_typed= _usual_sel=0
}

zle -N _usual_finish

_usual_key() {
  emulate -L zsh
  local n=$#_usual_cmds m=$#_usual_matches
  if [[ -n $_usual_text && -z $BUFFER ]] && (( _usual_on && n )); then
    case $WIDGET in
      usual-next)     _usual_i=$(( _usual_i % n + 1 )); return 0 ;;
      usual-previous) (( _usual_i > 1 )) && { (( _usual_i-- )); return 0 } ;;
      usual-dismiss)  _usual_on=0; return 0 ;;
      usual-accept)   _usual_take "$_usual_cmds[_usual_i]"; return 0 ;;
    esac
  elif [[ -n $_usual_text && -n $BUFFER && $BUFFER == $_usual_typed ]] && (( m )); then
    case $WIDGET in
      usual-next)     (( _usual_sel < m )) && (( _usual_sel++ )); return 0 ;;
      usual-previous) (( _usual_sel > 0 )) && { (( _usual_sel-- )); return 0 } ;;
      usual-dismiss)  _usual_typing=0; return 0 ;;
      usual-accept)   (( _usual_sel > 0 )) && { _usual_take "$_usual_matches[_usual_sel]"; return 0 } ;;
    esac
  fi
  local -A fallback=(usual-previous up-line-or-history usual-next down-line-or-history usual-accept expand-or-complete)
  local orig=${_usual_orig[${(V)KEYS}]:-$fallback[$WIDGET]}
  [[ -n $orig ]] || return 0
  # Clear first: some widgets (history-substring-search) redraw and wait for a key.
  # If the key changed nothing, nothing redraws, so put the hint back.
  local was=$BUFFER had=$_usual_text ret
  [[ -n $had ]] && _usual_clear
  zle $orig
  ret=$?
  [[ -n $had && $BUFFER == $was && -z $POSTDISPLAY ]] && { _usual_draw; zle -R }
  return ret
}
_usual_take() {
  BUFFER=$1 CURSOR=$#BUFFER
  (( _usual_run )) && { zle accept-line; return }
  (( ${+widgets[autosuggest-fetch]} )) && zle autosuggest-fetch
}
zle -N usual-accept _usual_key
zle -N usual-next _usual_key
zle -N usual-previous _usual_key
zle -N usual-dismiss _usual_key

_zsh_autosuggest_strategy_usual() {
  emulate -L zsh
  typeset -g suggestion=${_usual_cmds[(r)${(b)1}?*]}
}

_usual_keyname() {
  case $1 in
    '^I') REPLY=tab ;; '^M') REPLY=enter ;; '^[') REPLY=esc ;; '^?'|'^H') REPLY=bksp ;; ' ') REPLY=space ;;
    '^[[A'|'^[OA') REPLY=↑ ;; '^[[B'|'^[OB') REPLY=↓ ;; '^[[C'|'^[OC') REPLY=→ ;; '^[[D'|'^[OD') REPLY=← ;;
    '^['?) REPLY=alt-$1[3] ;; '^'?) REPLY=ctrl-${(L)1[2]} ;; *) REPLY=$1 ;;
  esac
}

# Ctrl-C skips the line-finish hook, so the INT trap clears the hint.
# Bindings and traps are read via a file: $(...) forks, about 1 ms each.
_usual_bind() {
  emulate -L zsh
  add-zsh-hook -d precmd _usual_bind
  # After mise, direnv and asdf change $PATH: hints depend on which programs are installed.
  precmd_functions=(${precmd_functions:#_usual_precmd} _usual_precmd)
  local tmp=${TMPDIR:-/tmp}/usual.$UID.$$ out line seq widget REPLY
  if { bindkey -M main; print -r -- '-- traps'; trap } >| $tmp 2>/dev/null; then
    out=$(<$tmp)
    zf_rm -f -- $tmp 2>/dev/null || command rm -f -- $tmp
  else
    out=$(bindkey -M main; print -r -- '-- traps'; trap)
  fi
  local -a lines=("${(@f)out}") words
  local split=${lines[(i)-- traps]} key
  local -A bound mine
  for line in ${(M)lines[1,split-1]:#(\"(\^I|\^\[[\[O][AB])\" *|* usual-*)}; do
    words=(${(z)line})
    (( $#words == 2 )) || continue
    key=${(Q)words[1]}
    bound[$key]=$words[2]
    [[ $words[2] == usual-* && -z $_usual_ours[$key] ]] && mine[${words[2]#usual-}]+="$key "
  done
  if (( $#mine )); then
    local km=emacs
    [[ -n ${(M)lines[1,split-1]:#\"\^\[\" vi-cmd-mode} ]] && km=viins
    for line in "${(@f)$(bindkey -d; for key in ${=mine}; do bindkey -M $km -- $key; done)}"; do
      words=(${(z)line})
      (( $#words == 2 )) && [[ $words[2] != undefined-key ]] && _usual_orig[${(Q)words[1]}]=$words[2]
    done
  fi
  for seq widget in '^I' accept '^[[B' next '^[OB' next '^[[A' previous '^[OA' previous; do
    (( ${+mine[$widget]} )) || [[ $bound[$seq] == usual-* ]] && continue
    [[ -n $bound[$seq] && $bound[$seq] != undefined-key ]] && _usual_orig[$seq]=$bound[$seq]
    bindkey -M main $seq usual-$widget
    _usual_ours[$seq]=1
  done
  local -A first=(accept '^I' next '^[[B' previous '^[[A')
  for widget in accept next previous; do
    _usual_keyname ${${=mine[$widget]}[1]:-$first[$widget]}
    _usual_names[$widget]=$REPLY
  done
  # powerlevel10k sets its own INT trap at every prompt, which calls this: clear the hint first.
  if (( ${+functions[_p9k_trapint]} )) && [[ $functions[_p9k_trapint] != *_usual_p9k_trapint* ]]; then
    functions[_usual_p9k_trapint]=$functions[_p9k_trapint]
    _p9k_trapint() { [[ -n $_usual_text ]] && zle && zle _usual_finish; _usual_p9k_trapint "$@" }
  fi
  if (( ! ${+functions[TRAPINT]} )) && [[ -z ${(M)lines[split+1,-1]:#*(INT|SIGINT)*} ]]; then
    setopt no_local_traps   # emulate -L would otherwise remove the trap when this returns
    TRAPINT() {
      [[ -n $_usual_text ]] && zle && zle _usual_finish
      return $(( 128 + $1 ))
    }
  fi
}

autoload -Uz add-zsh-hook add-zle-hook-widget
add-zsh-hook precmd _usual_precmd
add-zsh-hook precmd _usual_bind
add-zsh-hook preexec _usual_preexec
add-zle-hook-widget line-init _usual_draw
add-zle-hook-widget line-pre-redraw _usual_draw
add-zle-hook-widget line-finish _usual_finish
}
