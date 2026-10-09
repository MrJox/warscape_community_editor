#pragma once
// Small JSON value, parser and writer (RFC 8259) for talking to rpfm_server. Numbers are doubles; object member order is kept.
#include <string>
#include <utility>
#include <variant>
#include <vector>

namespace wce {

class Json {
public:
    using Array = std::vector<Json>;
    using Object = std::vector<std::pair<std::string, Json>>;

    Json() = default;
    Json(std::nullptr_t) {}
    Json(bool b) : v_(b) {}
    Json(double d) : v_(d) {}
    Json(int i) : v_(double(i)) {}
    Json(long long i) : v_(double(i)) {}
    Json(const char* s) : v_(std::string(s)) {}
    Json(std::string s) : v_(std::move(s)) {}
    Json(Array a) : v_(std::move(a)) {}
    Json(Object o) : v_(std::move(o)) {}

    static Json array() { return Json(Array{}); }
    static Json object() { return Json(Object{}); }

    static Json parse(const std::string& text); // throws std::runtime_error with the offset
    std::string dump() const;                    // compact

    bool is_null() const { return std::holds_alternative<std::monostate>(v_); }
    bool is_bool() const { return std::holds_alternative<bool>(v_); }
    bool is_number() const { return std::holds_alternative<double>(v_); }
    bool is_string() const { return std::holds_alternative<std::string>(v_); }
    bool is_array() const { return std::holds_alternative<Array>(v_); }
    bool is_object() const { return std::holds_alternative<Object>(v_); }

    // Accessors throw std::runtime_error on a type mismatch.
    bool as_bool() const;
    double as_number() const;
    const std::string& as_string() const;
    const Array& as_array() const;
    const Object& as_object() const;

    // Object member or nullptr; array element or nullptr.
    const Json* find(const std::string& key) const;
    const Json* at(size_t i) const;
    // Appends to an array / adds a member to an object (the value must already be of that kind).
    Json& push(Json v);
    Json& set(std::string key, Json v);

private:
    std::variant<std::monostate, bool, double, std::string, Array, Object> v_;
};

} // namespace wce
