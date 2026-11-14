[bits 16]
[org 0x0000]

kernel_main:
    ; Настраиваем сегменты ядра
    mov ax, 0x1000
    mov ds, ax
    mov es, ax

    ; Устанавливаем текстовый видеорежим 80x25, 16 цветов
    call set_mode_80x25

    ; Приветственный баннер pizdOS
    call show_banner

shell_loop:
    ; Вывод приглашения: "pizdOS> "
    mov si, prompt_str
    mov bl, [color_prompt]
    call print_colored

    ; Считываем строку в input_buffer
    call read_line

    ; Перевод строки
    call print_newline

    ; Если строка пустая - дальше
    cmp byte [input_buffer], 0
    je shell_loop

    ; Обработка команд
    call execute_command
    jmp shell_loop

; ========================================================
; РЕЖИМЫ ЭКРАНА И РАЗРЕШЕНИЕ
; ========================================================

set_mode_80x25:
    mov ax, 0x0003
    int 0x10
    ; Нормальный курсор
    mov ah, 0x01
    mov cx, 0x0607
    int 0x10
    mov byte [screen_rows], 25
    ret

set_mode_80x50:
    ; Сначала 80x25
    mov ax, 0x0003
    int 0x10
    ; Загрузка 8x8 шрифта (50 строк)
    mov ax, 0x1112
    xor bl, bl
    int 0x10
    ; Настройка курсора под 8x8 шрифт
    mov ah, 0x01
    mov cx, 0x0607
    int 0x10
    mov byte [screen_rows], 50
    ret

set_mode_80x28:
    mov ax, 0x0003
    int 0x10
    ; Загрузка 8x14 шрифта (28 строк)
    mov ax, 0x1111
    xor bl, bl
    int 0x10
    mov ah, 0x01
    mov cx, 0x0B0C
    int 0x10
    mov byte [screen_rows], 28
    ret

; ========================================================
; ВЫВОД ТЕКСТА
; ========================================================

; Печать строки из DS:SI с цветом BL (через BIOS int 10h)
print_colored:
    push ax
    push bx
    push cx
.loop:
    lodsb
    or al, al
    jz .done
    cmp al, 10
    je .handle_nl
    cmp al, 13
    je .handle_cr
    mov ah, 0x09        ; Вывод символа с цветом
    mov bh, 0           ; Страница 0
    mov cx, 1           ; 1 символ
    int 0x10
    call advance_cursor
    jmp .loop
.handle_cr:
    call carriage_return
    jmp .loop
.handle_nl:
    call line_feed
    jmp .loop
.done:
    pop cx
    pop bx
    pop ax
    ret

advance_cursor:
    push ax
    push bx
    push cx
    push dx
    mov ah, 0x03
    mov bh, 0
    int 0x10            ; DH = row, DL = col
    inc dl
    cmp dl, 80
    jb .set_pos
    xor dl, dl
    inc dh
    mov al, [screen_rows]
    cmp dh, al
    jb .set_pos
    call scroll_up
    dec al
    mov dh, al
.set_pos:
    mov ah, 0x02
    mov bh, 0
    int 0x10
    pop dx
    pop cx
    pop bx
    pop ax
    ret

carriage_return:
    push ax
    push bx
    push dx
    mov ah, 0x03
    mov bh, 0
    int 0x10
    xor dl, dl
    mov ah, 0x02
    mov bh, 0
    int 0x10
    pop dx
    pop bx
    pop ax
    ret

line_feed:
    push ax
    push bx
    push dx
    mov ah, 0x03
    mov bh, 0
    int 0x10
    inc dh
    mov al, [screen_rows]
    cmp dh, al
    jb .set_pos
    call scroll_up
    dec al
    mov dh, al
.set_pos:
    mov ah, 0x02
    mov bh, 0
    int 0x10
    pop dx
    pop cx
    pop bx
    pop ax
    ret

scroll_up:
    push ax
    push bx
    push cx
    push dx
    mov ah, 0x06
    mov al, 1
    mov bh, [color_text]
    xor cx, cx
    mov dl, 79
    mov dh, [screen_rows]
    dec dh
    int 0x10
    pop dx
    pop cx
    pop bx
    pop ax
    ret

print_newline:
    push si
    mov si, newline_str
    mov bl, [color_text]
    call print_colored
    pop si
    ret

; ========================================================
; ВВОД С КЛАВИАТУРЫ (ПОФИКШЕННЫЙ И НЕУБИВАЕМЫЙ BACKSPACE)
; ========================================================
read_line:
    push ax
    push bx
    push cx
    push dx
    mov di, input_buffer
    xor cx, cx          ; CX = текущая длина строки в буфере

.read_char:
    mov ah, 0x00
    int 0x16            ; AL = ASCII, AH = scancode

    ; Нажат Enter (ASCII 13)
    cmp al, 13
    je .done_input

    ; Нажат Backspace (ASCII 8 или scancode 0x0E или ASCII 127)
    cmp al, 8
    je .handle_backspace
    cmp al, 127
    je .handle_backspace
    cmp ah, 0x0E
    je .handle_backspace

    ; Игнорируем непечатные символы
    cmp al, 32
    jb .read_char
    cmp al, 126
    ja .read_char

    ; Защита от переполнения буфера (макс 60 символов)
    cmp cx, 60
    jae .read_char

    ; Записываем символ в буфер
    mov [di], al
    inc di
    inc cx

    ; Выводим символ с текущим цветом текста
    mov bl, [color_text]
    push ax
    mov ah, 0x09
    mov bh, 0
    push cx
    mov cx, 1
    int 0x10
    pop cx
    call advance_cursor
    pop ax
    jmp .read_char

.handle_backspace:
    ; ЕСЛИ СТРОКА ПУСТАЯ — СТРОГО ИГНОРИРУЕМ! НИКАКОГО УДАЛЕНИЯ PROMPT!
    cmp cx, 0
    jbe .read_char

    ; Уменьшаем размер и откатываем указатель
    dec di
    dec cx

    ; Стандартный надежный метод стирания в BIOS Teletype:
    ; 1) Сдвиг назад (BS = 8)
    mov ah, 0x0E
    mov al, 8
    int 0x10
    ; 2) Затираем пробелом (' ')
    mov ah, 0x0E
    mov al, ' '
    int 0x10
    ; 3) Сдвиг назад (BS = 8)
    mov ah, 0x0E
    mov al, 8
    int 0x10

    jmp .read_char

.done_input:
    mov byte [di], 0    ; Завершающий ноль
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ========================================================
; БАННЕР И ОФОРМЛЕНИЕ
; ========================================================
show_banner:
    mov si, banner_art
    mov bl, 0x0C        ; Ярко-красный
    call print_colored

    mov si, banner_desc
    mov bl, 0x0E        ; Ярко-желтый
    call print_colored
    ret

; ========================================================
; ПАРСИНГ И ВЫПОЛНЕНИЕ КОМАНД
; ========================================================
execute_command:
    ; Команда: help / huelp
    mov si, input_buffer
    mov di, cmd_help
    call strcmp
    jz do_help

    mov si, input_buffer
    mov di, cmd_huelp
    call strcmp
    jz do_help

    ; Команда: clear / nahuy
    mov si, input_buffer
    mov di, cmd_clear
    call strcmp
    jz do_clear

    mov si, input_buffer
    mov di, cmd_nahuy
    call strcmp
    jz do_clear

    ; Команда: res / screen
    mov si, input_buffer
    mov di, cmd_res
    call strstartswith
    jz do_res

    mov si, input_buffer
    mov di, cmd_screen
    call strstartswith
    jz do_res_screen

    ; Команда: echo / pizdi
    mov si, input_buffer
    mov di, cmd_echo
    call strstartswith
    jz do_echo

    mov si, input_buffer
    mov di, cmd_pizdi
    call strstartswith
    jz do_echo_pizdi

    ; Команда: info / poyasni
    mov si, input_buffer
    mov di, cmd_info
    call strcmp
    jz do_info

    mov si, input_buffer
    mov di, cmd_poyasni
    call strcmp
    jz do_info

    ; Команда: time / vremya
    mov si, input_buffer
    mov di, cmd_time
    call strcmp
    jz do_time

    mov si, input_buffer
    mov di, cmd_vremya
    call strcmp
    jz do_time

    ; Команда: beep / pikni
    mov si, input_buffer
    mov di, cmd_beep
    call strcmp
    jz do_beep

    mov si, input_buffer
    mov di, cmd_pikni
    call strcmp
    jz do_beep

    ; Команда: color / mast
    mov si, input_buffer
    mov di, cmd_color
    call strstartswith
    jz do_color

    mov si, input_buffer
    mov di, cmd_mast
    call strstartswith
    jz do_mast

    ; Команда: matrix / matrica
    mov si, input_buffer
    mov di, cmd_matrix
    call strcmp
    jz do_matrix

    mov si, input_buffer
    mov di, cmd_matrica
    call strcmp
    jz do_matrix

    ; Команда: panic / pizdec
    mov si, input_buffer
    mov di, cmd_panic
    call strcmp
    jz do_panic

    mov si, input_buffer
    mov di, cmd_pizdec
    call strcmp
    jz do_panic

    ; Команда: reboot / vpizdu
    mov si, input_buffer
    mov di, cmd_reboot
    call strcmp
    jz do_reboot

    mov si, input_buffer
    mov di, cmd_vpizdu
    call strcmp
    jz do_reboot

    ; Неизвестная команда
    mov si, err_unknown
    mov bl, 0x0C
    call print_colored
    ret

; ========================================================
; РЕАЛИЗАЦИЯ КОМАНД
; ========================================================

do_help:
    mov si, msg_help
    mov bl, 0x0A
    call print_colored
    ret

do_clear:
    mov al, [screen_rows]
    cmp al, 50
    je .clr50
    cmp al, 28
    je .clr28
    call set_mode_80x25
    ret
.clr50:
    call set_mode_80x50
    ret
.clr28:
    call set_mode_80x28
    ret

do_res:
    ; Пропускаем "res"
    mov si, input_buffer + 3
    jmp handle_res_arg

do_res_screen:
    ; Пропускаем "screen"
    mov si, input_buffer + 6
    jmp handle_res_arg

handle_res_arg:
    call skip_spaces
    lodsb
    or al, al
    jz .show_usage

    cmp al, '1'
    je .set25
    cmp al, '2'
    je .set50
    cmp al, '3'
    je .set28

    ; Проверяем ввод "50"
    cmp al, '5'
    je .set50
    cmp al, '2'
    je .set25

.show_usage:
    mov si, msg_res_info
    mov bl, 0x0E
    call print_colored
    ret

.set25:
    call set_mode_80x25
    mov si, msg_res_25_ok
    mov bl, 0x0A
    call print_colored
    ret

.set50:
    call set_mode_80x50
    mov si, msg_res_50_ok
    mov bl, 0x0A
    call print_colored
    ret

.set28:
    call set_mode_80x28
    mov si, msg_res_28_ok
    mov bl, 0x0A
    call print_colored
    ret

do_echo:
    mov si, input_buffer + 5
    call skip_spaces
    mov bl, [color_text]
    call print_colored
    call print_newline
    ret

do_echo_pizdi:
    mov si, input_buffer + 6
    call skip_spaces
    mov bl, [color_text]
    call print_colored
    call print_newline
    ret

do_info:
    mov si, msg_info
    mov bl, 0x0B
    call print_colored

    ; Текущее разрешение
    mov si, msg_curr_res
    mov bl, 0x0E
    call print_colored
    mov dl, [screen_rows]
    xor dh, dh
    call print_number
    mov si, msg_rows_suffix
    mov bl, 0x0E
    call print_colored
    call print_newline

    ; RAM
    clc
    int 0x12
    mov dx, ax
    mov si, msg_ram
    mov bl, 0x0E
    call print_colored
    call print_number
    mov si, msg_kb
    mov bl, 0x0E
    call print_colored
    call print_newline
    ret

do_time:
    mov ah, 0x02
    int 0x1A
    jc .err_rtc

    mov si, msg_time_prefix
    mov bl, 0x0E
    call print_colored

    mov al, ch
    call print_bcd
    mov al, ':'
    call print_char_raw

    mov al, cl
    call print_bcd
    mov al, ':'
    call print_char_raw

    mov al, dh
    call print_bcd
    call print_newline
    ret

.err_rtc:
    mov si, msg_rtc_err
    mov bl, 0x0C
    call print_colored
    ret

do_beep:
    mov si, msg_beep
    mov bl, 0x0D
    call print_colored

    in al, 0x61
    or al, 3
    out 0x61, al

    mov cx, 0x000F
    mov dx, 0x4240
    mov ah, 0x86
    int 0x15

    in al, 0x61
    and al, 0xFC
    out 0x61, al
    ret

do_color:
    mov si, input_buffer + 6
    call apply_color
    ret

do_mast:
    mov si, input_buffer + 5
    call apply_color
    ret

apply_color:
    call skip_spaces
    lodsb
    or al, al
    jz .done
    call hex_char_to_val
    mov [color_text], al
    mov si, msg_color_ok
    mov bl, [color_text]
    call print_colored
.done:
    ret

hex_char_to_val:
    cmp al, '9'
    jbe .digit
    or al, 0x20
    sub al, 'a' - 10
    ret
.digit:
    sub al, '0'
    ret

do_matrix:
    call do_clear
    mov si, msg_matrix_exit
    mov bl, 0x02
    call print_colored

.matrix_loop:
    mov ah, 0x01
    int 0x16
    jnz .matrix_exit

    xor ax, ax
    int 0x1A

    mov ax, dx
    xor dx, dx
    mov bx, 80
    div bx
    push dx

    in al, 0x40
    xor ah, ah
    mov bl, [screen_rows]
    dec bl
    dec bl
    div bl
    inc ah
    mov dh, ah
    pop ax
    mov dl, al

    mov ah, 0x02
    mov bh, 0
    int 0x10

    in al, 0x40
    and al, 0x3F
    add al, 33

    mov ah, 0x09
    mov bh, 0
    mov bl, 0x0A
    mov cx, 1
    int 0x10

    mov cx, 0x0000
    mov dx, 0x2000
    mov ah, 0x86
    int 0x15

    jmp .matrix_loop

.matrix_exit:
    mov ah, 0x00
    int 0x16
    call do_clear
    ret

do_panic:
    mov ah, 0x06
    xor al, al
    mov bh, 0x1F
    xor cx, cx
    mov dl, 79
    mov dh, [screen_rows]
    dec dh
    int 0x10

    mov ah, 0x02
    mov bh, 0
    mov dx, 0x0202
    int 0x10

    mov si, msg_bsod
    mov bl, 0x1F
    call print_colored

    mov ah, 0x00
    int 0x16
    call do_clear
    ret

do_reboot:
    mov si, msg_reboot
    mov bl, 0x0C
    call print_colored

    mov cx, 0x000F
    mov dx, 0x4240
    mov ah, 0x86
    int 0x15

    mov al, 0xFE
    out 0x64, al
    jmp 0xFFFF:0x0000

; ========================================================
; СЛУЖЕБНЫЕ ФУНКЦИИ
; ========================================================

skip_spaces:
.loop:
    cmp byte [si], ' '
    jne .done
    inc si
    jmp .loop
.done:
    ret

strcmp:
    push si
    push di
.loop:
    mov al, [si]
    mov bl, [di]
    cmp al, bl
    jne .diff
    or al, al
    jz .equal
    inc si
    inc di
    jmp .loop
.equal:
    pop di
    pop si
    xor ax, ax
    ret
.diff:
    pop di
    pop si
    mov ax, 1
    ret

strstartswith:
    push si
    push di
.loop:
    mov bl, [di]
    or bl, bl
    jz .matched
    mov al, [si]
    cmp al, bl
    jne .nomatch
    inc si
    inc di
    jmp .loop
.matched:
    pop di
    pop si
    xor ax, ax
    ret
.nomatch:
    pop di
    pop si
    mov ax, 1
    ret

print_bcd:
    push ax
    shr al, 4
    add al, '0'
    call print_char_raw
    pop ax
    and al, 0x0F
    add al, '0'
    call print_char_raw
    ret

print_char_raw:
    push ax
    push bx
    push cx
    mov ah, 0x09
    mov bh, 0
    mov bl, [color_text]
    mov cx, 1
    int 0x10
    call advance_cursor
    pop cx
    pop bx
    pop ax
    ret

print_number:
    push ax
    push bx
    push cx
    push dx
    mov ax, dx
    xor cx, cx
    mov bx, 10
.divide_loop:
    xor dx, dx
    div bx
    push dx
    inc cx
    or ax, ax
    jnz .divide_loop
.print_loop:
    pop dx
    mov al, dl
    add al, '0'
    call print_char_raw
    loop .print_loop
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ========================================================
; ДАННЫЕ И СТРОКИ
; ========================================================
screen_rows:    db 25       ; Текущее число строк (25, 28 или 50)
color_text:     db 0x0A     ; Светло-зеленый
color_prompt:   db 0x0E     ; Желтый

newline_str:    db 13, 10, 0
prompt_str:     db 13, 10, "pizdOS> ", 0

banner_art:
    db 13, 10
    db "  ______ _____ ______ _____   ____   _____ ", 13, 10
    db " |  __ \_   _|___  /|  __ \ / __ \ / ____|", 13, 10
    db " | |__) || |    / / | |  | | |  | | (___  ", 13, 10
    db " |  ___/ | |   / /  | |  | | |  | |\___ \ ", 13, 10
    db " | |    _| |_ / /__ | |__| | |__| |____) |", 13, 10
    db " |_|   |_____/_____|_____/ \____/|_____/  ", 13, 10, 0

banner_desc:
    db " === [ pizdOS v1.0.0 (Bratukha Ultimate Edition) ] ===", 13, 10
    db " Nahuy Windows, nahuy Linux! Chistyi x86 assembler, blyat!", 13, 10
    db " Vvedi 'help' ili 'huelp' dlya spiska komand, yopta.", 13, 10, 0

msg_help:
    db "=== SPISOK CHETKIH KOMAND pizdOS ===", 13, 10
    db "  help / huelp      - Pokazat etu spravku nafig", 13, 10
    db "  res [1|2|3]       - Nastroika razresheniya ekrana (25, 50, 28 strok)", 13, 10
    db "  clear / nahuy     - Ochistit ekran k chertej materi", 13, 10
    db "  echo / pizdi <msg>- Povtorit vash gniloy bazar", 13, 10
    db "  info / poyasni    - Poyasnit za sistemu, pamyat i razreshenie", 13, 10
    db "  time / vremya     - Realnoe vremya iz CMOS", 13, 10
    db "  beep / pikni      - Podat zvuk cherez PC speaker", 13, 10
    db "  color / mast <hex>- Smenit mast (1=Sin, 2=Zel, 4=Kras, E=Zhelt)", 13, 10
    db "  matrix / matrica  - Zalech na dno v Matrice (lyubaya knopka dlya vyhoda)", 13, 10
    db "  panic / pizdec    - Sinij ekran smerti (BSOD pizdOS)", 13, 10
    db "  reboot / vpizdu   - Perezagruzit komp k huam", 13, 10, 0

msg_res_info:
    db "=== NASTROYKA RAZRESHENIYA EKRANA ===", 13, 10
    db "  res 1  (ili res 25) - Standart 80x25 (klassika, bolshie bukvy)", 13, 10
    db "  res 2  (ili res 50) - Vysokie Chetkoe 80x50 (chotkiy melkiy shrift!)", 13, 10
    db "  res 3  (ili res 28) - Srednee 80x28 (EGA 8x14)", 13, 10, 0

msg_res_25_ok:      db "Vklyuchen rezhim 80x25!", 13, 10, 0
msg_res_50_ok:      db "Vklyucheno vysokoe razreshenie 80x50, bratishka!", 13, 10, 0
msg_res_28_ok:      db "Vklyuchen rezhim 80x28!", 13, 10, 0

msg_info:
    db "=== POYASNENIE ZA pizdOS ===", 13, 10
    db " Razrabotchiki : Bratukha, Lowyk & Antigravity AI", 13, 10
    db " Arhitektura   : 16-bit Real Mode x86 ASM", 13, 10
    db " Stabilnost    : 100% zheleznaya, bez tormozov", 13, 10, 0

msg_curr_res:       db " Tekuschee razreshenie: 80x", 0
msg_rows_suffix:    db " strok", 0
msg_ram:            db " Dostupnaya pamyat RAM: ", 0
msg_kb:             db " KB", 0
msg_time_prefix:    db "Vremya po poniyatiyam: ", 0
msg_rtc_err:        db "Chasy sbilis, pizdec!", 13, 10, 0
msg_beep:           db "Biiiip, naahuy!", 13, 10, 0
msg_color_ok:       db "Mast smenena uspeshno, bratishka!", 13, 10, 0
msg_matrix_exit:    db "MATRICA ZAPUSCHENA. Nazhmi lyubuyu knopku chtoby vyyti...", 13, 10, 0
msg_reboot:         db "Perezagruzka, vsem spasibo, vse svobodny...", 13, 10, 0

msg_bsod:
    db "================================================================================", 13, 10
    db "                         CRITICAL PIZDEC ERROR (BSOD)                           ", 13, 10
    db "================================================================================", 13, 10
    db "  Proizoshla neobratimaya huynya v yadre pizdOS v1.0.0                           ", 13, 10
    db "  Oshibka: 0xDEADBEEF (KTO-TO NAZHAT KNOPKU NE TUDA)                            ", 13, 10
    db "                                                                                ", 13, 10
    db "  * Posidite, pokurite, podumayte nad svoim povedeniem.                         ", 13, 10
    db "  * Vot do chego dohodyat shutochki s operatsionkami.                           ", 13, 10
    db "                                                                                ", 13, 10
    db "  Nazhmite lyubuyu klavishu chtoby vernutsya v pizdOS...                         ", 13, 10
    db "================================================================================", 0

err_unknown:
    db "Slysh, net takoy komandy! Vvedi 'help', ne tupi.", 13, 10, 0

; Имена команд
cmd_help:       db "help", 0
cmd_huelp:      db "huelp", 0
cmd_res:        db "res", 0
cmd_screen:     db "screen", 0
cmd_clear:      db "clear", 0
cmd_nahuy:      db "nahuy", 0
cmd_echo:       db "echo ", 0
cmd_pizdi:      db "pizdi ", 0
cmd_info:       db "info", 0
cmd_poyasni:    db "poyasni", 0
cmd_time:       db "time", 0
cmd_vremya:     db "vremya", 0
cmd_beep:       db "beep", 0
cmd_pikni:      db "pikni", 0
cmd_color:      db "color ", 0
cmd_mast:       db "mast ", 0
cmd_matrix:     db "matrix", 0
cmd_matrica:    db "matrica", 0
cmd_panic:      db "panic", 0
cmd_pizdec:     db "pizdec", 0
cmd_reboot:     db "reboot", 0
cmd_vpizdu:     db "vpizdu", 0

; Буфер ввода
input_buffer:   times 64 db 0

; Добиваем ядро до 16 КБ
times 16384 - ($ - $$) db 0
