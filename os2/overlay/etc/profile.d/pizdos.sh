export PATH="/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:$PATH"
export PS1='\[\033[1;31m\]pizdOS-2.1\[\033[0m\]:\[\033[1;32m\]\w\[\033[0m\]# '
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"
export DISPLAY="${DISPLAY:-:0}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-piz-0}"

alias nahuy='clear'
alias poyasni='pizfetch'
alias ui='pizsoft'
alias ebash='piz-get ebash'
alias snosi='piz-get snosi'
alias obnovi='piz-get obnovi'
alias vpizdu='poweroff'

if [ -t 0 ]; then
    /usr/bin/pizfetch
    TTY_DEV="$(tty 2>/dev/null || true)"
    if [ "$TTY_DEV" = "/dev/tty1" ] && [ -e /dev/fb0 ] && [ ! -f /tmp/.pizsoft_noauto ] && [ -z "$PIZSOFT_NO_AUTOUI" ]; then
        touch /tmp/.pizsoft_noauto
        echo -e "\033[1;36m>>> Avtozapusk Pizsoft UI 2.1(00) na /dev/fb0 (X11 :0 + Wayland piz-0)... <<<\033[0m"
        /usr/bin/pizsoft
    else
        echo -e "\033[1;33m>>> Vvedi 'pizsoft' ili 'ui' chtoby otkryt rabochiy stol Pizsoft UI! <<<\033[0m\n"
    fi
fi
