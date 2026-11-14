[bits 16]
[org 0x7c00]

start:
    ; Обнуляем сегментные регистры
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7c00
    sti

    mov [boot_drive], dl    ; Сохраняем номер диска от BIOS

    ; Печатаем сообщение о загрузке
    mov si, msg_loading
    call print_string

    ; Сброс дискового контроллера
    xor ax, ax
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    ; Читаем ядро из секторов 2..33 (16 КБ ядра) по адресу 0x1000:0x0000
    mov ax, 0x1000
    mov es, ax
    xor bx, bx              ; ES:BX = 0x1000:0x0000

    mov ah, 0x02            ; Чтение секторов
    mov al, 32              ; 32 сектора = 16 КБ
    mov ch, 0               ; Дорожка 0
    mov cl, 2               ; Сектор 2
    mov dh, 0               ; Головка 0
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    ; Прыгаем в ядро pizdOS!
    jmp 0x1000:0x0000

disk_error:
    mov si, msg_disk_error
    call print_string
    cli
    hlt

print_string:
    lodsb
    or al, al
    jz .done
    mov ah, 0x0E
    mov bh, 0
    int 0x10
    jmp print_string
.done:
    ret

boot_drive: db 0
msg_loading: db "Zagruzka pizdOS, suka...", 13, 10, 0
msg_disk_error: db "Oshibka diska, pizdec!", 13, 10, 0

times 510 - ($ - $$) db 0
dw 0xAA55
