
interface KinoFile {
    name: string,
    size: number,
    dir?: KinoFile[],
}

export default KinoFile;