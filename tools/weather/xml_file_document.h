#pragma once
// A weather file as a document: .environment (battle), .lighting (campaign, porthole), weather_definitions.xml, vfx/*.xml. The lossless XmlDoc is
// the model; every edit is a command on the document's stack, so undo and redo, merged drags and the modified marker come from the shared core.
// Saving writes the original bytes with only the edited spans changed (XmlDoc::save), as a loose file under the output folder keeping the in-pack path.
#include "editor/document.h"
#include "formats/xml_doc.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class XmlFileDocument : public Document {
public:
    struct Source {
        std::string vfs_path;                 // in-pack path, lower case; empty for a file opened from disk
        std::filesystem::path disk;           // file it was read from
        std::string label;                    // pack name or "loose"
    };
    XmlFileDocument(Source source, const std::vector<uint8_t>& bytes, std::filesystem::path output_dir);

    std::string title() const override;                 // the file name
    std::string kind() const override;                  // environment, lighting, weather_definitions, vfx, xml
    bool modified() const override { return xml_.modified(); }
    bool can_save() const override { return true; }
    bool save(const SaveTarget& target, std::string* message) override;
    void publish(RenderScene& scene, LayerId layer) override { (void)scene; (void)layer; }   // lights the scene through the weather tool, owns no entities
    OutlinerNode outline() const override;
    void inspect(const SourceRef& item, Inspector& inspector) override;
    std::string describe(const SourceRef& item) const override;
    std::string source_key() const override { return source_.vfs_path; }

    const XmlDoc& xml() const { return xml_; }
    const Source& source() const { return source_; }
    std::filesystem::path default_save_path() const;
    void set_output_dir(const std::filesystem::path& dir) { output_dir_ = dir; }
    bool is_environment() const;                         // .environment or .lighting: the kinds that light the scene

    // ---- edits (each is one command)
    // Sets an attribute to `raw` (already entity-encoded, as it is stored). `gesture` merges the commands of one drag into one entry (0: never merge).
    void set_attr(int elem, size_t attr, const std::string& raw, uint64_t gesture = 0);
    // Parses the displayed value of an attribute, replaces it, formats it back in the number style of the original text.
    void set_attr_text(int elem, size_t attr, const std::string& text, uint64_t gesture = 0);
    enum class Op { Remove, Duplicate, MoveUp, MoveDown, AddChild, AddAttr, RemoveAttr };
    // Structural edit; returns the element to select afterwards (-1: none) and, on failure, false with the reason.
    bool structural(Op op, int elem, size_t attr, const std::string& name, const std::string& value, int* select_after, std::string* error);
    // Back to the file as first loaded (also undoes structural edits), clears the history.
    void revert_all();

private:
    friend class XmlAttrCommand;
    friend class XmlSnapshotCommand;
    Source source_;
    XmlDoc xml_;
    std::filesystem::path output_dir_;
};

} // namespace wce
