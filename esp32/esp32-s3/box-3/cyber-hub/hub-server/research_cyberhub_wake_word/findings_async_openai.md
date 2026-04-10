# async-openai v0.27 Transcription Findings

The correct method/field for the initial prompt in `async-openai` crate v0.27 transcription requests is `.prompt()`.

## Details
- **Crate Version**: 0.27.0
- **Struct**: `CreateTranscriptionRequestArgs` (Builder)
- **Method**: `.prompt(value: Into<String>)`
- **Underlying Struct**: `CreateTranscriptionRequest`
- **Field**: `prompt`
