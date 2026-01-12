# Saorsa WebRTC Kotlin Bindings

Kotlin bindings for the Saorsa WebRTC library, providing a native Kotlin API for Android and JVM applications.

## ⚠️ Production Readiness

**Current Status: Mock Implementation**

The Kotlin bindings currently use a **mock/stub implementation** for development and testing purposes. The bindings are production-ready in terms of API design and error handling, but the underlying WebRTC functionality uses simplified stubs.

### Mock vs Real Modes

- **Mock Mode (Current)**: Simulated call states and transitions, no real media processing
- **Real Mode (Planned)**: Full WebRTC implementation with actual media capture and transmission

**Use for**: Development, API integration, testing UI flows  
**Not ready for**: Production video/audio calls

## Features

- ✅ Idiomatic Kotlin API
- ✅ Automatic resource management with AutoCloseable
- ✅ Comprehensive error handling with sealed classes
- ✅ Full test coverage
- ✅ Android and JVM support
- ✅ Uses JNA for native interop
- ⚠️ Mock WebRTC implementation (real implementation pending)

## Installation

### Gradle (Kotlin DSL)

```kotlin
dependencies {
    implementation("com.saorsalabs:saorsa-webrtc-kotlin:0.2.1")
}
```

### Gradle (Groovy)

```groovy
dependencies {
    implementation 'com.saorsalabs:saorsa-webrtc-kotlin:0.2.1'
}
```

### Maven

```xml
<dependency>
    <groupId>com.saorsalabs</groupId>
    <artifactId>saorsa-webrtc-kotlin</artifactId>
    <version>0.2.1</version>
</dependency>
```

## Usage

```kotlin
import com.saorsalabs.webrtc.SaorsaWebRTC
import com.saorsalabs.webrtc.CallState

// Initialize the service (AutoCloseable)
SaorsaWebRTC("alice-bob-charlie-david").use { service ->
    // Initiate a call
    val callId = service.call("bob-smith-jones-wilson")
    println("Call initiated: $callId")
    
    // Check call state
    val state = service.getCallState(callId)
    println("Call state: $state")
    
    // End the call
    service.endCall(callId)
} // Automatically closed
```

## API Reference

### `SaorsaWebRTC`

#### Constructor

```kotlin
SaorsaWebRTC(identity: String)
```

Initialize the WebRTC service with a four-word identity string.

**Throws:** `IllegalArgumentException` if identity is empty

#### Methods

```kotlin
fun call(peer: String): String
```

Initiate a call to a peer. Returns a unique call ID.

**Parameters:**
- `peer`: The peer's identity string

**Returns:** Call ID for tracking this call

**Throws:** 
- `IllegalArgumentException` if peer is empty
- `SaorsaError.InvalidHandle` if service not initialized
- `SaorsaError.ConnectionFailed` if call initiation fails

```kotlin
fun getCallState(callId: String): CallState
```

Get the current state of a call.

**Parameters:**
- `callId`: The call ID from `call()`

**Returns:** Current `CallState`

**Throws:** 
- `IllegalArgumentException` if callId is empty
- `SaorsaError.InvalidHandle` if service not initialized
- `SaorsaError.CallNotFound` if call doesn't exist

```kotlin
fun endCall(callId: String)
```

End an active call.

**Parameters:**
- `callId`: The call ID to end

**Throws:** 
- `IllegalArgumentException` if callId is empty
- `SaorsaError.InvalidHandle` if service not initialized
- `SaorsaError.CallNotFound` if call doesn't exist

```kotlin
override fun close()
```

Release resources. Called automatically when using `use` block.

### `CallState`

Enum representing the state of a call:

- `CONNECTING` - Call is being established
- `ACTIVE` - Call is connected
- `ENDED` - Call has ended normally
- `FAILED` - Call failed

### `SaorsaError`

Sealed class hierarchy for errors:

- `InvalidParameter(message)` - Invalid input parameter
- `OutOfMemory` - Memory allocation failed
- `NotInitialized` - Service not initialized
- `AlreadyInitialized` - Service already initialized
- `ConnectionFailed` - Connection could not be established
- `InternalError` - Internal error occurred
- `InvalidHandle` - Invalid service handle
- `CallNotFound` - Specified call not found

## Android Permissions

Add to your `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.RECORD_AUDIO" />
<uses-permission android:name="android.permission.CAMERA" />
<uses-permission android:name="android.permission.MODIFY_AUDIO_SETTINGS" />
```

## Testing

Run tests:

```bash
./gradlew test
```

## Requirements

- JVM 17+
- Android API 24+ (for Android)
- Kotlin 1.9+

## Switching Between Mock and Real Modes

### Current Implementation

The bindings currently link to the FFI layer which uses mock implementations. To verify the mode:

```kotlin
// All calls currently use mock implementation
SaorsaWebRTC("test-identity").use { service ->
    // This creates a mock service - no real WebRTC operations occur
    val callId = service.call("peer-identity")
}
```

### Future Real Mode (Planned)

When real WebRTC implementation is integrated:

1. The FFI layer will connect to actual WebRTC core
2. No API changes required - same Kotlin interface
3. Real media capture and transmission will occur
4. Feature flag or build configuration may control mode selection

**Migration Path**: When upgrading to real mode, existing code will work without modification. Only behavior changes from simulated to actual media processing.

### Conditional Behavior

For apps that need to handle both modes:

```kotlin
// Future: Check mode at runtime
val isMockMode = service.isMockMode() // Planned API
if (isMockMode) {
    // Show warning or limit features
    println("Running in development/mock mode")
}
```

## License

AGPL-3.0
