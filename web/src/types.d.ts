declare module '@cornerstonejs/codec-openjpeg/decodewasmjs' { const factory: (options?: any) => Promise<any>; export default factory; }
interface Window { showDirectoryPicker?: (options: { mode: 'read' }) => Promise<FileSystemDirectoryHandle> }
