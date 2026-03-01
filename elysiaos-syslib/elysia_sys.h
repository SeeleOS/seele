#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>
#include "stdint.h"
#include "stddef.h"

extern "C" {

intptr_t c_sys_exit();

intptr_t c_sys_set_fs(uint64_t addr);

intptr_t c_sys_get_fs();

intptr_t c_sys_set_gs(uint64_t addr);

intptr_t c_sys_allocate_mem_pages(uint64_t pages, uint64_t flags);

intptr_t c_sys_allocate_mem(uint64_t len, uint64_t flags);

intptr_t c_sys_get_process_id();

intptr_t c_sys_get_thread_id();

intptr_t c_sys_read_object(uint64_t index, uint8_t *ptr, uintptr_t len);

intptr_t c_sys_write_object(uint64_t index, const uint8_t *ptr, uintptr_t len);

intptr_t c_sys_configurate_object(uint64_t index, uint64_t request_num, uint8_t *ptr);

}  // extern "C"
