BUILD_HOST=$(shell rustc -vV | grep "host: " | sed "s/host: //g")
all:
	@echo "Building MCMU for host $(BUILD_HOST)"
	cargo build --release --target $(BUILD_HOST)
clean:
	@echo "Cleaning..."
	rm -rf ./build ./target
ios: i386-ios x86_64-ios armv7-ios armv7s-ios arm64-ios
	@echo "Combine built iOS binaries"
	@mkdir -p ./build
	lipo -create ./target/*-ios/release/mcmu -output ./build/mcmu-ios-universal
	@echo "Signing with specific entitlements"
	ldid -S./build-misc/ios-entitlements.plist ./build/mcmu-ios-universal
	codesign --force --deep --entitlements ./build-misc/ios-entitlements.plist -s - ./build/mcmu-ios-universal
i386-ios:
	@echo "Building MCMU for iOS Simulator (i386)"
	cargo build --release --target i386-apple-ios
x86_64-ios:
	@echo "Building MCMU for iOS Simulator (x86_64)"
	cargo build --release --target x86_64-apple-ios
armv7-ios:
	@echo "Building MCMU for iOS (armv7)"
	cargo build --release --target armv7-apple-ios
armv7s-ios:
	@echo "Building MCMU for iOS (armv7s)"
	cargo build --release --target armv7s-apple-ios
arm64-ios:
	@echo "Building MCMU for iOS (arm64)"
	cargo build --release --target aarch64-apple-ios
macos: x86_64-darwin arm64-darwin
	@echo "Combine built macOS binaries"
	@mkdir -p ./build
	lipo -create ./target/*-darwin/release/mcmu -output ./build/mcmu-macos-universal
x86_64-darwin:
	@echo "Building MCMU for macOS (intel, x86_64)"
	cargo build --release --target x86_64-apple-darwin
arm64-darwin:
	@echo "Building MCMU for macOS (arm64)"
	cargo build --release --target aarch64-apple-darwin