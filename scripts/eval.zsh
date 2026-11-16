#!/usr/bin/env zsh
# scripts/eval.zsh CORPUS: keys it takes to reach each project's dev, test, build and lint command.
emulate -L zsh
setopt extendedglob
corpus=${1:?usage: scripts/eval.zsh CORPUS}
here=${0:A:h:h}
work=$(mktemp -d)
trap 'rm -rf $work' EXIT
cargo build --quiet --release --manifest-path $here/Cargo.toml && mkdir -p $work/bin && cp $here/target/release/usual $work/bin/ || exit 1
for tool in npm pnpm yarn bun deno just make task mise composer cargo go uv poetry pytest docker git cp mix mvn gradle; do
  [[ -x $work/bin/$tool ]] || { print '#!/bin/sh' >$work/bin/$tool; chmod +x $work/bin/$tool }
done

typeset -ga _usual_cmds _usual_matches
typeset -gA _usual_starts
typeset -g _usual_typed= _usual_sel=0 BUFFER=
eval "$(sed -n '/^_usual_match() {/,/^}/p' $here/usual.plugin.zsh)"

keys_by() {
  local target=$1 text=$2 n best=999 at cost
  for (( n = 1; n <= $#text && n <= 12; n++ )); do
    BUFFER=$text[1,n] _usual_typed=
    _usual_match
    at=${_usual_matches[(ie)$target]}
    (( at <= $#_usual_matches )) || continue
    cost=$(( n + at + 1 ))
    (( cost < best )) && best=$cost
  done
  REPLY=$best
}

median() { local -a s=(${(on)@}); REPLY=${s[$(( ($# + 1) / 2 ))]} }

typeset -A hint intent command hand first total
integer projects=0
for repo in $corpus/*(N-/); do
  out=("${(@f)$(HOME=$work/home XDG_CACHE_HOME=$work/cache PATH=$work/bin $work/bin/usual suggest $repo:A 2>/dev/null)}")
  n=${out[2]:-0}
  [[ $n == <1-> ]] || continue
  _usual_cmds=() _usual_starts=()
  for (( k = 1; k <= n; k++ )); do
    [[ $out[2+n+k] == (dependencies not installed|no .env yet|submodules not checked out|*changed since*) ]] || _usual_cmds+=($out[2+k])
  done
  (( $#_usual_cmds )) || continue
  (( projects++ ))
  for word in dev start test build lint; do
    target=${_usual_cmds[(I)(* |)$word( *|)]}
    (( target )) || continue
    target=${_usual_cmds[(i)(* |)$word( *|)]}
    cmd=$_usual_cmds[target]
    total[$word]=$(( total[$word] + 1 ))
    hint[$word]+=" $(( target + 1 ))"
    (( target == 1 )) && first[$word]=$(( first[$word] + 1 ))
    keys_by $cmd $word;     intent[$word]+=" $REPLY"
    keys_by $cmd $cmd;      command[$word]+=" $REPLY"
    hand[$word]+=" $#cmd"
  done
done

print "$projects projects with commands, once set up; median keys to get each command on the line"
printf '%-7s %9s %14s %11s %12s %9s   %s\n' intent projects 'before typing' 'by intent' 'by command' 'by hand' 'offered first'
for word in dev start test build lint; do
  (( total[$word] )) || continue
  median ${=hint[$word]};    h=$(( REPLY - 1 ))
  median ${=intent[$word]};  i=$REPLY
  median ${=command[$word]}; c=$REPLY
  median ${=hand[$word]};    t=$REPLY
  printf '%-7s %9d %14d %11d %12d %9d   %d%%\n' $word $total[$word] $h $i $c $t $(( 100 * ${first[$word]:-0} / total[$word] ))
done
