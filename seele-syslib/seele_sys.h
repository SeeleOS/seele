#ifndef seele_syslib_h
#define seele_syslib_h

#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>
#include "stdint.h"
#include "stddef.h"

constexpr static const uint32_t seele_ICANON = 2;

constexpr static const uint32_t seele_ECHO = 8;

constexpr static const uint32_t seele_ECHOE = 16;

constexpr static const uint32_t seele_ECHOK = 32;

constexpr static const uint32_t seele_ECHONL = 64;

constexpr static const uintptr_t seele_NCCS = 32;

#endif  // seele_syslib_h
