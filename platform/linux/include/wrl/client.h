#pragma once
// Microsoft::WRL::ComPtr for the Linux build (DXVK-native ships the D3D headers but not WRL): the subset the editor uses.
#include <windows.h>

#include <cstddef>
#include <utility>

namespace Microsoft::WRL {

template <typename T>
class ComPtr {
public:
    using InterfaceType = T;

    ComPtr() = default;
    ComPtr(std::nullptr_t) {}
    template <typename U>
    ComPtr(U* p) : ptr_(p) { add_ref(); }
    ComPtr(const ComPtr& o) : ptr_(o.ptr_) { add_ref(); }
    template <typename U>
    ComPtr(const ComPtr<U>& o) : ptr_(o.Get()) { add_ref(); }
    ComPtr(ComPtr&& o) noexcept : ptr_(std::exchange(o.ptr_, nullptr)) {}
    template <typename U>
    ComPtr(ComPtr<U>&& o) noexcept : ptr_(o.Detach()) {}
    ~ComPtr() { release(); }

    ComPtr& operator=(std::nullptr_t) { release(); return *this; }
    ComPtr& operator=(T* p) { ComPtr(p).Swap(*this); return *this; }
    ComPtr& operator=(const ComPtr& o) { ComPtr(o).Swap(*this); return *this; }
    template <typename U>
    ComPtr& operator=(const ComPtr<U>& o) { ComPtr(o).Swap(*this); return *this; }
    ComPtr& operator=(ComPtr&& o) noexcept { ComPtr(std::move(o)).Swap(*this); return *this; }
    template <typename U>
    ComPtr& operator=(ComPtr<U>&& o) noexcept { ComPtr(std::move(o)).Swap(*this); return *this; }

    T* Get() const { return ptr_; }
    T* operator->() const { return ptr_; }
    explicit operator bool() const { return ptr_ != nullptr; }
    T* const* GetAddressOf() const { return &ptr_; }
    T** GetAddressOf() { return &ptr_; }
    T** ReleaseAndGetAddressOf() { release(); return &ptr_; }
    T** operator&() { return ReleaseAndGetAddressOf(); }

    unsigned long Reset() { return release(); }
    void Attach(T* p) { release(); ptr_ = p; }
    T* Detach() { return std::exchange(ptr_, nullptr); }
    void Swap(ComPtr& o) noexcept { std::swap(ptr_, o.ptr_); }

    template <typename U>
    HRESULT As(ComPtr<U>* out) const { return ptr_->QueryInterface(__uuidof(U), reinterpret_cast<void**>(out->ReleaseAndGetAddressOf())); }
    template <typename U>
    HRESULT As(U** out) const { return ptr_->QueryInterface(__uuidof(U), reinterpret_cast<void**>(out)); }   // `x.As(&other)`: operator& already released it
    template <typename U>
    HRESULT CopyTo(U** out) const { return ptr_->QueryInterface(__uuidof(U), reinterpret_cast<void**>(out)); }
    HRESULT CopyTo(T** out) const { add_ref(); *out = ptr_; return S_OK; }

private:
    void add_ref() const { if (ptr_) ptr_->AddRef(); }
    unsigned long release() {
        unsigned long n = 0;
        if (T* p = std::exchange(ptr_, nullptr)) n = p->Release();
        return n;
    }

    T* ptr_ = nullptr;
};

template <typename T, typename U>
bool operator==(const ComPtr<T>& a, const ComPtr<U>& b) { return a.Get() == b.Get(); }
template <typename T>
bool operator==(const ComPtr<T>& a, std::nullptr_t) { return a.Get() == nullptr; }

} // namespace Microsoft::WRL
