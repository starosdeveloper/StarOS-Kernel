# STAR OS Kernel FFI Integration Guide

Complete guide for integrating Kotlin/Native UI with STAR OS kernel through FFI layer.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Setup](#setup)
4. [Display API](#display-api)
5. [Input API](#input-api)
6. [Memory Management](#memory-management)
7. [IPC](#ipc)
8. [System Management](#system-management)
9. [Error Handling](#error-handling)
10. [Best Practices](#best-practices)

---

## Overview

The FFI (Foreign Function Interface) layer provides a C ABI bridge between the Rust kernel and Kotlin/Native UI code. All kernel functionality is exposed through safe, well-documented C functions.

### Key Features

- **Zero-copy buffer sharing** for graphics
- **Asynchronous DMA** for high-performance rendering
- **Multi-touch input** with gesture recognition
- **IPC** for process communication
- **Power management** integration
- **Comprehensive error handling**

---

## Architecture

```
┌─────────────────────────────────────────┐
│     Kotlin/Native UI Application        │
│  (Idiomatic Kotlin API - Task #9)      │
└─────────────────────────────────────────┘
                  │
                  │ Kotlin/Native cinterop
                  ▼
┌─────────────────────────────────────────┐
│         FFI Layer (C ABI)               │
│  display │ input │ ipc │ memory │ sys  │
└─────────────────────────────────────────┘
                  │
                  │ Internal Rust API
                  ▼
┌─────────────────────────────────────────┐
│          STAR OS Kernel                 │
│  display_server │ input │ process       │
└─────────────────────────────────────────┘
```

---

## Setup

### 1. Build Kernel with FFI

```bash
cd kernel
cargo build --release --target aarch64-unknown-none
```

This produces `libstaros_kernel.a` in `target/aarch64-unknown-none/release/`.

### 2. Generate Kotlin/Native Bindings

```bash
cd kotlin-ui
kotlinc-native -target linux_arm64 \
               -def staros_kernel.def \
               -o staros_kernel
```

This generates `staros_kernel.klib` with Kotlin bindings.

### 3. Initialize FFI Layer

```kotlin
import com.staros.kernel.ffi.*

fun main() {
    // Initialize FFI
    val result = staros_ffi_init()
    if (result != FFI_ERROR_SUCCESS) {
        error("Failed to initialize FFI")
    }
    
    // Your UI code here
    
    // Cleanup
    staros_ffi_shutdown()
}
```

---

## Display API

### Initialize Display

```kotlin
// Initialize display server
val fbAddr = 0x3C000000UL  // Framebuffer physical address
val width = 1920u
val height = 1080u
val stride = 1920u

val result = staros_display_init(fbAddr, width, height, stride)
if (result != FFI_ERROR_SUCCESS) {
    error("Failed to initialize display")
}
```

### Create and Manage Surfaces

```kotlin
// Create a solid color surface
val surface = FFISurface(
    x = 100u,
    y = 100u,
    width = 400u,
    height = 300u,
    color = staros_display_rgba(255u, 0u, 0u, 255u), // Red
    pixels = null,
    stride = 0u,
    blend_mode = FFI_BLEND_MODE_ALPHA,
    z_order = 10u,
    visible = true
)

val handleResult = staros_display_add_surface(surface)
if (handleResult.error == FFI_ERROR_SUCCESS) {
    val handle = handleResult.value
    
    // Move surface
    staros_display_move_surface(handle, 200u, 200u)
    
    // Hide surface
    staros_display_set_surface_visible(handle, false)
    
    // Remove surface
    staros_display_remove_surface(handle)
}
```

### Render Text

```kotlin
val text = "Hello, STAR OS!"
val textBytes = text.encodeToByteArray()

val textSurface = FFITextSurface(
    x = 50u,
    y = 50u,
    max_width = 800u,
    max_height = 100u,
    fg_color = staros_display_rgb(255u, 255u, 255u), // White
    bg_color = staros_display_rgba(0u, 0u, 0u, 128u), // Semi-transparent black
    font_scale = 2u,
    z_order = 20u,
    visible = true,
    text = textBytes.refTo(0),
    text_len = textBytes.size.toULong()
)

val handleResult = staros_display_add_text_surface(textSurface)
```

### Present Frame (Synchronous)

```kotlin
// Simple synchronous rendering
staros_display_present()
```

### Present Frame (Asynchronous DMA)

```kotlin
// High-performance async rendering
val cookieResult = staros_display_present_async(dmaOps, dmaChannel, tracker)
if (cookieResult.error == FFI_ERROR_SUCCESS) {
    val cookie = cookieResult.value
    
    // Do other work while DMA is in progress...
    
    // Wait for completion before next frame
    staros_display_wait_flip(cookie, dmaOps, dmaChannel)
}
```

---

## Input API

### Poll Input Events

```kotlin
val maxEvents = 64
val events = Array<FFIInputEvent>(maxEvents) { FFIInputEvent() }

val count = staros_input_poll_events(events.refTo(0), maxEvents.toULong())

for (i in 0 until count.toInt()) {
    val event = events[i]
    when (event.event_type) {
        FFI_INPUT_EVENT_MOUSE_MOVE -> {
            val data = event.data.mouse_move
            println("Mouse moved: dx=${data.dx}, dy=${data.dy}")
        }
        FFI_INPUT_EVENT_MOUSE_BUTTON -> {
            val data = event.data.mouse_button
            println("Mouse button: pressed=${data.pressed}, x=${data.x}, y=${data.y}")
        }
        FFI_INPUT_EVENT_TOUCH -> {
            val data = event.data.touch
            println("Touch: id=${data.id}, x=${data.x}, y=${data.y}, pressed=${data.pressed}")
        }
        FFI_INPUT_EVENT_KEY_DOWN -> {
            val data = event.data.key
            println("Key down: scancode=${data.scancode}")
        }
        else -> {}
    }
}
```

### Cursor Control

```kotlin
// Get cursor position
val cursorResult = staros_input_get_cursor_pos()
if (cursorResult.error == FFI_ERROR_SUCCESS) {
    val cursor = cursorResult.value
    println("Cursor at (${cursor.x}, ${cursor.y})")
}

// Set cursor position
staros_input_set_cursor_pos(100u, 100u)

// Hide cursor
staros_input_set_cursor_visible(false)
```

### Touch Input

```kotlin
val maxTouches = 10
val touches = Array<FFITouchData>(maxTouches) { FFITouchData() }

val count = staros_input_get_touches(touches.refTo(0), maxTouches.toULong())

for (i in 0 until count.toInt()) {
    val touch = touches[i]
    println("Touch ${touch.id}: (${touch.x}, ${touch.y}) pressed=${touch.pressed}")
}
```

### Gesture Recognition

```kotlin
// Detect swipe
val direction = ByteArray(1)
if (staros_input_detect_swipe(direction.refTo(0))) {
    when (direction[0].toInt()) {
        0 -> println("Swipe up")
        1 -> println("Swipe right")
        2 -> println("Swipe down")
        3 -> println("Swipe left")
    }
}

// Detect pinch
val scale = FloatArray(1)
if (staros_input_detect_pinch(scale.refTo(0))) {
    if (scale[0] > 1.0f) {
        println("Zoom in: ${scale[0]}")
    } else {
        println("Zoom out: ${scale[0]}")
    }
}
```

---

## Memory Management

### Allocate Memory

```kotlin
// Allocate buffer
val size = 4096uL
val align = 8uL
val ptr = staros_memory_alloc(size, align)

if (ptr != null) {
    // Use buffer...
    
    // Free buffer
    staros_memory_free(ptr, size, align)
}
```

### DMA Buffers (Zero-Copy)

```kotlin
// Allocate DMA-capable buffer
val bufferResult = staros_memory_alloc_dma_buffer(1920u * 1080u * 4u)
if (bufferResult.error == FFI_ERROR_SUCCESS) {
    val buffer = bufferResult.value
    
    println("DMA buffer: phys=0x${buffer.phys_addr.toString(16)}")
    println("            virt=0x${buffer.virt_addr.toString(16)}")
    println("            size=${buffer.size}")
    
    // Use buffer for graphics...
    
    // Flush cache before DMA
    staros_memory_cache_flush(buffer.virt_addr, buffer.size.toULong())
    
    // Free buffer
    staros_memory_free_dma_buffer(buffer)
}
```

### Memory Statistics

```kotlin
val statsResult = staros_memory_get_stats()
if (statsResult.error == FFI_ERROR_SUCCESS) {
    val stats = statsResult.value
    println("Memory: ${stats.used_bytes} / ${stats.total_bytes} bytes used")
    println("Free: ${stats.free_bytes} bytes")
    println("Largest free block: ${stats.largest_free_block} bytes")
}
```

---

## IPC

### Message Passing

```kotlin
// Create message queue
val queue = staros_ipc_create_queue()

// Send message
val msg = FFIMessage(
    sender = 1uL,
    msg_type = 42uL,
    data = longArrayOf(1, 2, 3, 4, 5, 6)
)
staros_ipc_send_message(queue, msg)

// Receive message
val msgResult = staros_ipc_receive_message(queue)
if (msgResult.error == FFI_ERROR_SUCCESS) {
    val receivedMsg = msgResult.value
    println("Received message type ${receivedMsg.msg_type} from ${receivedMsg.sender}")
}

// Cleanup
staros_ipc_destroy_queue(queue)
```

### Signals

```kotlin
// Create signal manager
val signalMgr = staros_ipc_create_signal_manager()

// Send signal
staros_ipc_send_signal(signalMgr, FFI_SIGNAL_INTERRUPT)

// Check for pending signals
if (staros_ipc_has_pending_signal(signalMgr)) {
    val signalResult = staros_ipc_get_pending_signal(signalMgr)
    if (signalResult.error == FFI_ERROR_SUCCESS) {
        println("Received signal: ${signalResult.value}")
    }
}

// Cleanup
staros_ipc_destroy_signal_manager(signalMgr)
```

### Shared Memory

```kotlin
// Create shared memory region
val shm = staros_ipc_create_shared_memory(0x10000000uL, 4096uL)

// Attach to shared memory
staros_ipc_attach_shared_memory(shm)

// Get info
val infoResult = staros_ipc_get_shared_memory_info(shm)
if (infoResult.error == FFI_ERROR_SUCCESS) {
    val info = infoResult.value
    println("Shared memory: base=0x${info.base.toString(16)}, size=${info.size}")
    println("Reference count: ${info.ref_count}")
}

// Detach
val lastRef = BooleanArray(1)
staros_ipc_detach_shared_memory(shm, lastRef.refTo(0))

if (lastRef[0]) {
    // Last reference, can destroy
    staros_ipc_destroy_shared_memory(shm)
}
```

---

## System Management

### Task Information

```kotlin
// Get current task ID
val taskId = staros_system_get_current_task_id()
println("Current task: $taskId")

// Get task info
val infoResult = staros_system_get_task_info(taskId)
if (infoResult.error == FFI_ERROR_SUCCESS) {
    val info = infoResult.value
    println("Task ${info.task_id}: priority=${info.priority}, state=${info.state}")
    println("CPU time: ${info.cpu_time_us} μs")
}

// Get all tasks
val maxTasks = 64
val taskIds = LongArray(maxTasks)
val count = staros_system_get_all_tasks(taskIds.refTo(0), maxTasks.toULong())
println("Active tasks: $count")
```

### Power Management

```kotlin
// Get power state
val state = staros_system_get_power_state()
println("Power state: $state")

// Enter idle state (CPU sleep until interrupt)
staros_system_set_power_state(FFI_POWER_STATE_IDLE)

// Shutdown system
staros_system_shutdown()

// Reboot system
staros_system_reboot()
```

### Watchdog

```kotlin
// Enable watchdog (5 second timeout)
staros_system_watchdog_enable(5000u)

// Pet watchdog in main loop
while (running) {
    staros_system_watchdog_pet()
    
    // Do work...
    
    Thread.sleep(1000)
}

// Disable watchdog
staros_system_watchdog_disable()
```

### System Statistics

```kotlin
// Uptime
val uptimeMs = staros_system_get_uptime_ms()
println("Uptime: ${uptimeMs / 1000} seconds")

// CPU usage
val cpuUsage = staros_system_get_cpu_usage()
println("CPU usage: $cpuUsage%")

// Context switches
val switches = staros_system_get_context_switches()
println("Context switches: $switches")

// Interrupts
val interrupts = staros_system_get_interrupt_count()
println("Interrupts: $interrupts")
```

### Debug Logging

```kotlin
fun debugPrint(msg: String) {
    val bytes = msg.encodeToByteArray()
    staros_system_debug_print(bytes.refTo(0), bytes.size.toULong())
}

debugPrint("Hello from Kotlin/Native!")
```

---

## Error Handling

All FFI functions return either `FFIError` or `FFIResult<T>`. Always check for errors:

```kotlin
fun checkError(error: FFIError, operation: String) {
    if (error != FFI_ERROR_SUCCESS) {
        val errorName = when (error) {
            FFI_ERROR_INVALID_PARAMETER -> "Invalid parameter"
            FFI_ERROR_NOT_INITIALIZED -> "Not initialized"
            FFI_ERROR_OUT_OF_MEMORY -> "Out of memory"
            FFI_ERROR_RESOURCE_EXHAUSTED -> "Resource exhausted"
            FFI_ERROR_NOT_FOUND -> "Not found"
            FFI_ERROR_PERMISSION_DENIED -> "Permission denied"
            FFI_ERROR_BUSY -> "Busy"
            FFI_ERROR_TIMEOUT -> "Timeout"
            FFI_ERROR_IO_ERROR -> "I/O error"
            else -> "Unknown error"
        }
        error("$operation failed: $errorName")
    }
}

// Usage
val result = staros_display_init(fbAddr, width, height, stride)
checkError(result, "Display initialization")
```

---

## Best Practices

### 1. Initialize FFI First

Always call `staros_ffi_init()` before any other FFI functions.

### 2. Check All Errors

Never ignore error codes. Always check `FFIError` and `FFIResult.error`.

### 3. Free Resources

Always free allocated resources (memory, queues, surfaces, etc.).

### 4. Cache Flush for DMA

Always flush cache before DMA operations:

```kotlin
staros_memory_cache_flush(buffer.virt_addr, buffer.size.toULong())
```

### 5. Pet the Watchdog

If watchdog is enabled, pet it regularly in your main loop.

### 6. Use Async Rendering

For best performance, use async DMA rendering:

```kotlin
// Frame N
val cookie = staros_display_present_async(...)

// Render frame N+1 while DMA copies N
prepareNextFrame()

// Wait for N to complete
staros_display_wait_flip(cookie, ...)
```

### 7. Batch Input Events

Poll multiple events at once instead of one at a time:

```kotlin
val events = Array<FFIInputEvent>(64) { FFIInputEvent() }
val count = staros_input_poll_events(events.refTo(0), 64uL)
```

### 8. Zero-Copy Graphics

Use DMA buffers for large graphics data to avoid copying:

```kotlin
val buffer = staros_memory_alloc_dma_buffer(size)
// Render directly into buffer.virt_addr
// Use buffer.phys_addr for DMA
```

---

## Next Steps

- **Task #9**: Create idiomatic Kotlin wrapper library
- **Task #10**: Add comprehensive documentation and examples

For more information, see:
- `kernel/src/ffi/mod.rs` - FFI layer documentation
- `kernel/src/ffi/types.rs` - Type definitions
- Individual FFI modules for detailed API docs

---

**STAR OS Kernel FFI v0.1.0**  
*Built for OBT Fall 2026*
