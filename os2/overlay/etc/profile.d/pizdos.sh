export PATH="/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:$PATH"
export PS1='\[\033[1;31m\]pizdOS-2.0\[\033[0m\]:\[\033[1;32m\]\w\[\033[0m\]# '

alias nahuy='clear'
alias poyasni='pizfetch'
alias ui='pizsoft'
alias ebash='piz-get ebash'
alias snosi='piz-get snosi'
alias obnovi='piz-get obnovi'
alias vpizdu='poweroff'

if [ -t 0 ]; then
    /usr/bin/pizfetch
    echo -e "\033[1;33m>>> Vvedi 'pizsoft' ili 'ui' chtoby otkryt rabochiy stol Pizsoft UI! <<<\033[0m\n"
fi
