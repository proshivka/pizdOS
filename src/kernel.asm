[bits 16]
[org 0x0000]

kernel_main:
    ; Настраиваем сегменты ядра
    mov ax, 0x1000
    mov ds, ax
    mov es, ax

    ; Устанавливаем текстовый видеорежим 80x25, 16 цветов
    mov ax, 0x0003
    int 0x10

    ; Прячем курсор или делаем нормальным (скан-линии 6-7)
    mov ah, 0x01
    mov cx, 0x0607
    int 0x10

    ; Приветственный баннер pizdOS
    call show_banner

shell_loop:
    ; Вывод приглашения: "pizdOS> "
    mov si, prompt_str
    mov bl, [color_prompt]
    call print_colored

    ; Считываем строку в input_buffer
    mov di, input_buffer
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
; ПОДПРОГРАММЫ ВЫВОДА И КЛАВИАТУРЫ
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
    ; Двигаем курсор вправо
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

; Движение курсора на 1 позицию вперед
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
    cmp dh, 25
    jb .set_pos
    ; Скролл экрана
    call scroll_up
    mov dh, 24
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
    cmp dh, 25
    jb .set_pos
    call scroll_up
    mov dh, 24
.set_pos:
    mov ah, 0x02
    mov bh, 0
    int 0x10
    pop dx
    pop bx
    pop ax
    ret

scroll_up:
    push ax
    push bx
    push cx
    push dx
    mov ah, 0x06
    mov al, 1           ; Скролл на 1 строку
    mov bh, [color_text] ; Атрибут заполнения
    xor cx, cx          ; Верхний левый угол (0, 0)
    mov dx, 0x184F      ; Нижний правый угол (24, 79)
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

; Чтение строки с эхо и поддержкой Backspace
read_line:
    push ax
    push bx
    push cx
    push dx
    xor cx, cx          ; Длина введенного
.read_char:
    mov ah, 0x00
    int 0x16            ; Ждем нажатия клавиши: AL = ASCII, AH = scancode

    cmp al, 13          ; Enter
    je .done_input

    cmp al, 8           ; Backspace
    je .handle_backspace

    cmp al, 32          ; Печатные символы
    jb .read_char
    cmp al, 126
    ja .read_char

    ; Проверка переполнения буфера (макс 64 символа)
    cmp cx, 63
    jae .read_char

    ; Сохраняем символ
    mov [di], al
    inc di
    inc cx

    ; Выводим символ на экран
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
    jcxz .read_char     ; Буфер пуст - ничего не стираем
    dec di
    dec cx

    ; Смещаем курсор назад
    mov ah, 0x03
    mov bh, 0
    int 0x10            ; DH = row, DL = col
    dec dl
    mov ah, 0x02
    mov bh, 0
    int 0x10

    ; Стираем пробелом
    mov ah, 0x09
    mov al, ' '
    mov bh, 0
    push cx
    mov cx, 1
    int 0x10
    pop cx
    jmp .read_char

.done_input:
    mov byte [di], 0    ; Терминирующий ноль
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
    ; Команда: help
    mov si, input_buffer
    mov di, cmd_help
    call strcmp
    jz do_help

    ; Команда: huelp
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
    mov bl, 0x0C        ; Красный
    call print_colored
    ret

; ========================================================
; РЕАЛИЗАЦИЯ КОМАНД
; ========================================================

do_help:
    mov si, msg_help
    mov bl, 0x0A        ; Зеленый
    call print_colored
    ret

do_clear:
    mov ax, 0x0003
    int 0x10
    ret

do_echo:
    ; Пропускаем "echo " (5 символов)
    mov si, input_buffer + 5
    call skip_spaces
    mov bl, [color_text]
    call print_colored
    call print_newline
    ret

do_echo_pizdi:
    ; Пропускаем "pizdi " (6 символов)
    mov si, input_buffer + 6
    call skip_spaces
    mov bl, [color_text]
    call print_colored
    call print_newline
    ret

do_info:
    mov si, msg_info
    mov bl, 0x0B        ; Бирюзовый
    call print_colored

    ; Получаем размер доступной базовой памяти (в КБ) через int 12h
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
    ; Читаем RTC время: AH = 02h, INT 1Ah
    ; Возвращает: CH = часы (BCD), CL = минуты (BCD), DH = секунды (BCD)
    mov ah, 0x02
    int 0x1A
    jc .err_rtc

    mov si, msg_time_prefix
    mov bl, 0x0E
    call print_colored

    ; Часы
    mov al, ch
    call print_bcd
    mov al, ':'
    call print_char_raw

    ; Минуты
    mov al, cl
    call print_bcd
    mov al, ':'
    call print_char_raw

    ; Секунды
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

    ; Включаем PC Speaker
    in al, 0x61
    or al, 3
    out 0x61, al

    ; Простая задержка
    mov cx, 0x000F
    mov dx, 0x4240
    mov ah, 0x86
    int 0x15

    ; Выключаем спикер
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
    ; Преобразуем hex символ в число
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
    or al, 0x20     ; к нижнему регистру
    sub al, 'a' - 10
    ret
.digit:
    sub al, '0'
    ret

do_matrix:
    ; Очистка экрана
    mov ax, 0x0003
    int 0x10

    mov si, msg_matrix_exit
    mov bl, 0x02
    call print_colored

.matrix_loop:
    ; Проверяем, нажата ли клавиша
    mov ah, 0x01
    int 0x16
    jnz .matrix_exit

    ; Получаем случайный символ и позицию через timer tick
    xor ax, ax
    int 0x1A        ; DX = ticks

    ; Случайная колонка 0..79
    mov ax, dx
    xor dx, dx
    mov bx, 80
    div bx          ; DX = колонка 0..79
    push dx         ; сохраняем колонку

    ; Случайная строка 1..24
    in al, 0x40     ; Чтение таймера PIT
    xor ah, ah
    mov bl, 23
    div bl
    inc ah          ; AH = строка 1..23
    mov dh, ah
    pop ax          ; Восстанавливаем колонку
    mov dl, al

    ; Ставим курсор
    mov ah, 0x02
    mov bh, 0
    int 0x10

    ; Случайный зеленый символ
    in al, 0x40
    and al, 0x3F
    add al, 33      ; Символы от '!' до 'Z'

    mov ah, 0x09
    mov bh, 0
    mov bl, 0x0A    ; Ярко-зеленый
    mov cx, 1
    int 0x10

    ; Короткая пауза
    mov cx, 0x0000
    mov dx, 0x2000
    mov ah, 0x86
    int 0x15

    jmp .matrix_loop

.matrix_exit:
    ; Очищаем нажатую клавишу
    mov ah, 0x00
    int 0x16
    ; Очищаем экран
    mov ax, 0x0003
    int 0x10
    ret

do_panic:
    ; Синий экран смерти pizdOS
    mov ah, 0x06
    xor al, al
    mov bh, 0x1F        ; Белый на синем
    xor cx, cx
    mov dx, 0x184F
    int 0x10

    ; Курсор в (2, 2)
    mov ah, 0x02
    mov bh, 0
    mov dx, 0x0202
    int 0x10

    mov si, msg_bsod
    mov bl, 0x1F
    call print_colored

    ; Ждем любой клавиши
    mov ah, 0x00
    int 0x16

    ; Возвращаем нормальный экран
    mov ax, 0x0003
    int 0x10
    ret

do_reboot:
    mov si, msg_reboot
    mov bl, 0x0C
    call print_colored

    ; Задержка 1 секунда
    mov cx, 0x000F
    mov dx, 0x4240
    mov ah, 0x86
    int 0x15

    ; Перезагрузка через контроллер клавиатуры 8042
    mov al, 0xFE
    out 0x64, al

    ; Если не сработало - прыгаем на BIOS POST
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

; Сравнение строк DS:SI и ES:DI
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
    xor ax, ax          ; ZF = 1
    ret
.diff:
    pop di
    pop si
    mov ax, 1           ; ZF = 0
    ret

; Проверка начинается ли DS:SI с ES:DI
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
    xor ax, ax          ; ZF = 1
    ret
.nomatch:
    pop di
    pop si
    mov ax, 1           ; ZF = 0
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
color_text:     db 0x0A     ; Светло-зеленый по умолчанию
color_prompt:   db 0x0E     ; Желтый по умолчанию

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
    db "  clear / nahuy     - Ochistit ekran k chertej materi", 13, 10
    db "  echo / pizdi <msg>- Povtorit vash gniloy bazar", 13, 10
    db "  info / poyasni    - Poyasnit za sistemu i pamyat", 13, 10
    db "  time / vremya     - Realnoe vremya iz CMOS", 13, 10
    db "  beep / pikni      - Podat zvuk cherez PC speaker", 13, 10
    db "  color / mast <hex>- Smenit mast (1=Sin, 2=Zel, 4=Kras, E=Zhelt)", 13, 10
    db "  matrix / matrica  - Zalech na dno v Matrice (lyubaya knopka dlya vyhoda)", 13, 10
    db "  panic / pizdec    - Sinij ekran smerti (BSOD pizdOS)", 13, 10
    db "  reboot / vpizdu   - Perezagruzit komp k huam", 13, 10, 0

msg_info:
    db "=== POYASNENIE ZA pizdOS ===", 13, 10
    db " Razrabotchiki : Bratukha & Antigravity AI", 13, 10
    db " Arhitektura   : 16-bit Real Mode x86 ASM", 13, 10
    db " Stabilnost    : 100% zheleznaya, bez tormozov", 13, 10, 0

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

; Добиваем размер ядра до 16 КБ (32 сектора = 16384 байт)
times 16384 - ($ - $$) db 0
