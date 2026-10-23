// A C interface to the parts of the engine that turn a drawable (an image and
// its directives) into pixels. Everything here calls the engine's own code,
// nothing is reimplemented.
#include "StarBuffer.hpp"
#include "StarDirectives.hpp"
#include "StarImage.hpp"
#include "StarPackedAssetSource.hpp"

#include <cstdlib>
#include <cstring>

using namespace Star;

namespace {
thread_local std::string lastError;

struct Handle {
  DirectivesGroup group;
  Image result;
};

template <typename F>
auto guarded(F &&f, decltype(f()) failure) -> decltype(f()) {
  try {
    return f();
  } catch (std::exception const &e) {
    lastError = e.what();
  } catch (...) {
    lastError = "unknown exception";
  }
  return failure;
}
} // namespace

extern "C" {

char const *sb_last_error() { return lastError.c_str(); }

// directives

void *sb_directives_new(char const *data, size_t len) {
  return guarded(
      [&]() -> void * {
        return new Handle{DirectivesGroup(String(data, len)), Image()};
      },
      nullptr);
}

void sb_directives_free(void *handle) { delete static_cast<Handle *>(handle); }

// Loads (parses) every operation. The engine does this lazily, on first use,
// for strings of 1000 characters and more. Returns the number of operations or
// -1.
long sb_directives_load(void *handle) {
  return guarded(
      [&]() -> long {
        auto &group = static_cast<Handle *>(handle)->group;
        group.imageReferences();
        long count = 0;
        group.forEach(
            [&](Directives::Entry const &entry, Directives const &directives) {
              if (entry.loadOperation(*directives).is<ErrorImageOperation>())
                throw StarException(
                    "directives contain an operation that fails to parse");
              ++count;
            });
        return count;
      },
      -1);
}

// Applies the directives to an RGBA image the way Assets does for a drawable.
// The result stays valid until the next call.
uint8_t const *sb_directives_apply(void *handle, uint8_t const *rgba,
                                   uint32_t width, uint32_t height,
                                   uint32_t *outWidth, uint32_t *outHeight) {
  return guarded(
      [&]() -> uint8_t const * {
        auto *h = static_cast<Handle *>(handle);
        Image input(width, height, PixelFormat::RGBA32);
        std::memcpy(input.data(), rgba, size_t(width) * height * 4);
        h->result = h->group.applyNewImage(input);
        if (h->result.pixelFormat() != PixelFormat::RGBA32)
          h->result = h->result.convert(PixelFormat::RGBA32);
        *outWidth = h->result.width();
        *outHeight = h->result.height();
        return h->result.data();
      },
      nullptr);
}

// The same, but one operation after the other, without the engine reordering or
// combining any of them, and only the first `limit`
uint8_t const *sb_directives_apply_sequential(void *handle, uint8_t const *rgba,
                                              uint32_t width, uint32_t height,
                                              size_t limit, uint32_t *outWidth,
                                              uint32_t *outHeight) {
  return guarded(
      [&]() -> uint8_t const * {
        auto *h = static_cast<Handle *>(handle);
        Image image(width, height, PixelFormat::RGBA32);
        std::memcpy(image.data(), rgba, size_t(width) * height * 4);
        size_t index = 0;
        h->group.forEach([&](Directives::Entry const &entry,
                             Directives const &directives) {
          if (index++ >= limit)
            return;
          ImageOperation const &operation = entry.loadOperation(*directives);
          if (operation.is<ErrorImageOperation>())
            throw StarException(
                "directives contain an operation that fails to parse");
          if (!operation.is<NullImageOperation>())
            processImageOperation(operation, image);
        });
        h->result = std::move(image);
        if (h->result.pixelFormat() != PixelFormat::RGBA32)
          h->result = h->result.convert(PixelFormat::RGBA32);
        *outWidth = h->result.width();
        *outHeight = h->result.height();
        return h->result.data();
      },
      nullptr);
}

// images

// Decodes a png to RGBA8. Free the data with sb_free.
uint8_t *sb_png_decode(uint8_t const *data, size_t len, uint32_t *width,
                       uint32_t *height) {
  return guarded(
      [&]() -> uint8_t * {
        Image image = Image::readPng(
            make_shared<Buffer>(ByteArray((char const *)data, len)));
        if (image.pixelFormat() != PixelFormat::RGBA32)
          image = image.convert(PixelFormat::RGBA32);
        *width = image.width();
        *height = image.height();
        size_t size = size_t(*width) * *height * 4;
        auto *out = (uint8_t *)std::malloc(size ? size : 1);
        std::memcpy(out, image.data(), size);
        return out;
      },
      nullptr);
}

// asset packs

void *sb_pak_open(char const *path) {
  return guarded([&]() -> void * { return new PackedAssetSource(path); },
                 nullptr);
}

void sb_pak_close(void *pak) { delete static_cast<PackedAssetSource *>(pak); }

// Reads a file from the pack. Free the data with sb_free.
uint8_t *sb_pak_read(void *pak, char const *path, size_t *len) {
  return guarded(
      [&]() -> uint8_t * {
        ByteArray bytes = static_cast<PackedAssetSource *>(pak)->read(path);
        *len = bytes.size();
        auto *out = (uint8_t *)std::malloc(bytes.size() ? bytes.size() : 1);
        std::memcpy(out, bytes.ptr(), bytes.size());
        return out;
      },
      nullptr);
}

void sb_free(void *ptr) { std::free(ptr); }
}
