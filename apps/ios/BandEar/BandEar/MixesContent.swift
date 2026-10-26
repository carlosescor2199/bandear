import BandEarScreens
import BandEarSession
import SwiftUI
import UIKit

/// The Mixes tab as Android's `SavedMixesScreen` lays it out, rendered natively: the title and its
/// introduction, the save card with what the last action did under it, then the saved mixes, each
/// with Recall and a ⋮ menu holding Rename and Delete, or the empty state while none is saved.
///
/// The presentational half: it shows the values it is given and reports actions through its
/// closures, with every word from ``SavedMixesWords``. What is typed and which dialog is open are
/// its own state, as on Android. Rename and Delete each ask first, in a dialog, so a stray tap on
/// stage never loses a mix.
struct MixesContent: View {
    /// The saved mixes' names, in the order they were saved.
    let names: [String]
    let activeName: String?
    let notice: SavedMixesNotice?
    /// The notice's number, new with every notice an action gives, even in the last one's words.
    let noticeNumber: Int
    /// Saves the current mix under the name typed; says whether it was saved.
    let save: @MainActor (String) -> Bool
    let recall: @MainActor (String) -> Void
    let rename: @MainActor (_ from: String, _ to: String) -> Void
    let delete: @MainActor (String) -> Void

    /// What the save field holds.
    @State private var typedName = ""
    @FocusState private var saveFieldFocused: Bool
    /// The mix whose rename dialog is open, and the name typed in it.
    @State private var renaming: String?
    @State private var newName = ""
    /// The mix whose delete dialog is open.
    @State private var deleting: String?

    var body: some View {
        List {
            introSection
            saveSection
            listSection
        }
        .scrollDismissesKeyboard(.interactively)
        .navigationTitle(SavedMixesWords.screenTitle)
        .alert(
            SavedMixesWords.renameTitle(renaming ?? ""), isPresented: shown($renaming),
            presenting: renaming
        ) { from in
            TextField(SavedMixesWords.renameFieldLabel, text: $newName)
                .textInputAutocapitalization(.sentences)
            Button(SavedMixesWords.rename) { confirmRename(from) }
                .disabled(!SavedMixesWords.canRename(from: from, to: newName))
            Button(SavedMixesWords.cancel, role: .cancel) {}
        }
        // Android reads the notice out as a polite live region whenever its words change. Here every
        // notice is read out, even in the last one's words (a second blank save, a re-save): its
        // number tells them apart. Both are in the value watched, so the words read out are the ones
        // handed to the closure.
        .onChange(of: Announcement(number: noticeNumber, text: noticeText)) { announcement in
            if let text = announcement.text {
                UIAccessibility.post(notification: .announcement, argument: text)
            }
        }
    }

    /// What the last action did, in words, or nil.
    private var noticeText: String? {
        notice.flatMap(SavedMixesWords.notice)
    }

    /// A notice as VoiceOver reads it out: its words, and its number, which changes with each one.
    private struct Announcement: Equatable {
        let number: Int
        let text: String?
    }

    private var introSection: some View {
        Section {
            Text(SavedMixesWords.screenIntro)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
        }
    }

    /// The name field and Save, with what the last action did under them.
    private var saveSection: some View {
        Section {
            HStack(spacing: 8) {
                TextField(
                    SavedMixesWords.saveFieldLabel, text: $typedName,
                    prompt: Text(SavedMixesWords.saveFieldPlaceholder)
                )
                .textInputAutocapitalization(.sentences)
                .submitLabel(.done)
                .focused($saveFieldFocused)
                .onSubmit(saveTypedName)
                Button(SavedMixesWords.save, action: saveTypedName)
                    .buttonStyle(.borderedProminent)
            }
        } header: {
            Text(SavedMixesWords.saveCaption)
                .textCase(nil)
        } footer: {
            if let noticeText {
                Text(noticeText)
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.primary)
            }
        }
    }

    /// The saved mixes, or the empty state while none is saved.
    private var listSection: some View {
        Section {
            if names.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text(SavedMixesWords.emptyTitle)
                        .font(.headline)
                    Text(SavedMixesWords.emptyDetail)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
                .accessibilityElement(children: .combine)
            } else {
                // A name is saved once, so it tells the rows apart.
                ForEach(names, id: \.self) { name in
                    row(name)
                }
            }
        } header: {
            Text(SavedMixesWords.listTitle(count: names.count))
                .textCase(nil)
        } footer: {
            if !names.isEmpty {
                Text(SavedMixesWords.menuHint)
            }
        }
    }

    /// One saved mix: its name, "Active now" while it is the active one, Recall, and the ⋮ menu.
    private func row(_ name: String) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.body.weight(.semibold))
                    .lineLimit(1)
                if let detail = SavedMixesWords.detail(active: name == activeName) {
                    Text(detail)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(.tint)
                }
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 8)
            Button(SavedMixesWords.recall) { recall(name) }
                .buttonStyle(.bordered)
                .accessibilityLabel(SavedMixesWords.recallLabel(name))
            Menu {
                Button(SavedMixesWords.rename) { startRenaming(name) }
                Button(SavedMixesWords.delete, role: .destructive) { deleting = name }
            } label: {
                // The ⋮ the hint under the list names.
                Image(systemName: "ellipsis")
                    .rotationEffect(.degrees(90))
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            // Rename first and Delete second wherever the menu opens, as on Android; iOS otherwise
            // puts the first item nearest the button, so a menu opening upwards lists Delete first.
            .menuOrder(.fixed)
            .buttonStyle(.borderless)
            .accessibilityLabel(SavedMixesWords.moreActionsLabel(name))
            // Asked here, so the question points at the mix it is about.
            .confirmationDialog(
                SavedMixesWords.deleteTitle(name),
                isPresented: Binding(
                    get: { deleting == name },
                    set: { if !$0 { deleting = nil } }),
                titleVisibility: .visible
            ) {
                Button(SavedMixesWords.delete, role: .destructive) { confirmDelete(name) }
                Button(SavedMixesWords.cancel, role: .cancel) { deleting = nil }
            } message: {
                Text(SavedMixesWords.deleteMessage)
            }
        }
    }

    // MARK: Actions

    /// Saves under the typed name. A saved mix clears the field and leaves it, so it shows the
    /// cleared text; a refused name stays to be corrected (`nameAfterSave`).
    private func saveTypedName() {
        let saved = save(typedName)
        if saved { saveFieldFocused = false }
        typedName = SavedMixesWords.nameAfterSave(typed: typedName, saved: saved)
    }

    /// Opens the rename dialog with the mix's own name in the field, as Android's does.
    private func startRenaming(_ name: String) {
        newName = name
        renaming = name
    }

    /// Renames to the typed name without the spaces around it, once it can be confirmed.
    private func confirmRename(_ from: String) {
        if let to = SavedMixesWords.renameTarget(from: from, to: newName) {
            rename(from, to)
        }
    }

    /// Lets the delete dialog go, then deletes the mix. The dialog hangs from the mix's row, which
    /// the delete removes, so nothing else may close it; left open, a mix saved again under the
    /// same name would show it again.
    private func confirmDelete(_ name: String) {
        deleting = nil
        delete(name)
    }

    /// Shown while `item` holds a mix; closing the dialog lets it go.
    private func shown(_ item: Binding<String?>) -> Binding<Bool> {
        Binding(
            get: { item.wrappedValue != nil },
            set: { if !$0 { item.wrappedValue = nil } })
    }
}

#if DEBUG
    #Preview("Mixes · three mixes, one active") {
        NavigationStack {
            MixesContent(
                names: ["Soundcheck", "Ballads", "Main set"], activeName: "Ballads",
                notice: .recalled(name: "Ballads"), noticeNumber: 1, save: { _ in true },
                recall: { _ in }, rename: { _, _ in }, delete: { _ in })
        }
    }

    #Preview("Mixes · empty") {
        NavigationStack {
            MixesContent(
                names: [], activeName: nil, notice: nil, noticeNumber: 0, save: { _ in false },
                recall: { _ in }, rename: { _, _ in }, delete: { _ in })
        }
    }
#endif
