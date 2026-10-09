// Gate: the in-place structural edits of XmlDoc (splice, remove_element_inplace, duplicate_element_inplace, bake_pending) give the same text and the same
// element table as the functional ones that re-parse the document, and their records undo and redo exactly.
//   xml_splice_check <file.xml> [rounds]
#include "foundation/text_encoding.h"
#include "formats/xml_doc.h"

#include <cstdio>
#include <cstdlib>
#include <random>
#include <string>
#include <vector>

using namespace wce;

static int g_failed = 0;
#define CHECK(cond, what) \
    do { \
        if (!(cond)) { \
            std::printf("FAIL: %s\n", what); \
            ++g_failed; \
        } \
    } while (0)

static bool same_table(const XmlDoc& a, const XmlDoc& b, std::string* why) {
    if (a.elements().size() != b.elements().size()) { *why = "element count " + std::to_string(a.elements().size()) + " vs " + std::to_string(b.elements().size()); return false; }
    if (a.roots() != b.roots()) { *why = "roots"; return false; }
    for (size_t i = 0; i < a.elements().size(); ++i) {
        const XmlElement &x = a.elements()[i], &y = b.elements()[i];
        if (x.name != y.name || x.parent != y.parent || x.children != y.children || x.start_off != y.start_off || x.attrs_end != y.attrs_end || x.start_end != y.start_end ||
            x.end_off != y.end_off || x.self_closing != y.self_closing || x.attrs.size() != y.attrs.size()) {
            *why = "element " + std::to_string(i) + " <" + x.name + ">";
            return false;
        }
        for (size_t k = 0; k < x.attrs.size(); ++k)
            if (x.attrs[k].name != y.attrs[k].name || x.attrs[k].value_off != y.attrs[k].value_off || x.attrs[k].value_len != y.attrs[k].value_len || x.attrs[k].quote != y.attrs[k].quote) {
                *why = "attribute " + std::to_string(k) + " of element " + std::to_string(i);
                return false;
            }
    }
    return true;
}

int main(int argc, char** argv) {
    if (argc < 2) {
        std::printf("usage: xml_splice_check <file.xml> [rounds]\n");
        return 2;
    }
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    const int rounds = argc > 2 ? std::atoi(argv[2]) : 40;
    const std::vector<uint8_t> bytes = read_file(argv[1]);
    XmlDoc fast = XmlDoc::parse(bytes), slow = XmlDoc::parse(bytes);
    const XmlDoc original = XmlDoc::parse(bytes);
    std::mt19937 rng(12345);
    std::vector<std::vector<XmlSplice>> records;
    std::string why;
    auto pick = [&](const XmlDoc& d) {   // an element that is not the first root
        for (int tries = 0; tries < 50; ++tries) {
            const int e = int(rng() % d.elements().size());
            if (d.elements()[size_t(e)].parent >= 0) return e;
        }
        return -1;
    };
    for (int r = 0; r < rounds; ++r) {
        const int e = pick(fast);
        if (e < 0) break;
        const bool remove = (rng() % 3) == 0;
        fast.bake_pending();
        const size_t mark = fast.journal_mark();
        try {
            if (remove) {
                fast.remove_element_inplace(e);
                slow = slow.removed_element(e);
            } else {
                int copy = -1;
                const int ci = fast.duplicate_element_inplace(e);
                slow = slow.duplicated_element(e, &copy);
                CHECK(ci == copy, "the copy gets the same index");
            }
        } catch (const std::exception& ex) {
            std::printf("round %d: %s\n", r, ex.what());
            ++g_failed;
            break;
        }
        records.push_back(fast.journal_since(mark));
        fast.journal_forget(mark);
        // a value edit after the structural one, inside the same record
        if (!fast.elements().empty() && (rng() % 3) == 0) {
            const int t = int(rng() % fast.elements().size());
            if (!fast.elements()[size_t(t)].attrs.empty()) {
                const size_t m2 = fast.journal_mark();
                fast.set(t, fast.elements()[size_t(t)].attrs[0].name, "later" + std::to_string(r));
                slow.set(t, slow.elements()[size_t(t)].attrs[0].name, "later" + std::to_string(r));
                fast.bake_pending();
                for (auto& s : fast.journal_since(m2)) records.back().push_back(s);
                fast.journal_forget(m2);
                slow = XmlDoc::parse(slow.save());   // the functional edits bake by themselves; here the edit is baked so that the tables can be compared
            }
        }
        const bool text_same = fast.original_text() == slow.original_text();
        CHECK(text_same, "same text as the functional edit");
        if (!text_same) {
            std::printf("round %d (%s element %d)\n", r, remove ? "remove" : "duplicate", e);
            break;
        }
        CHECK(same_table(fast, slow, &why), why.c_str());
        if (!why.empty()) {
            std::printf("round %d: table differs: %s\n", r, why.c_str());
            break;
        }
    }
    std::printf("%zu structural edits applied\n", records.size());
    // undo everything, then redo everything
    try {
    for (size_t i = records.size(); i-- > 0;)
        for (size_t k = records[i].size(); k-- > 0;) fast.unapply_splice(records[i][k]);
    CHECK(fast.original_text() == original.original_text(), "undoing every record gives the original text");
    CHECK(same_table(fast, original, &why), ("undone table: " + why).c_str());
    CHECK(fast.structural_edits() == 0, "no structural edits remain after undoing them all");
    for (auto& rec : records)
        for (auto& s : rec) fast.apply_splice(s);
    CHECK(fast.original_text() == slow.original_text(), "redoing every record gives the edited text again");
    CHECK(same_table(fast, slow, &why), ("redone table: " + why).c_str());
    } catch (const std::exception& ex) {
        std::printf("undo / redo threw: %s\n", ex.what());
        ++g_failed;
    }
    std::printf(g_failed ? "FAILED\n" : "ok\n");
    return g_failed ? 1 : 0;
}
