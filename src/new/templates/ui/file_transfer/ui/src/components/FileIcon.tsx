import {  FaBoxArchive, FaCode, FaFile, FaFilePdf, FaFileZipper, FaFilm, FaFolder, FaImage, FaJs, FaMusic } from "react-icons/fa6";
import KinoFile from "../types/KinoFile";

export const FileIcon = (props: { file: KinoFile }) => {
    const { file } = props;
    const { name, dir } = file;
    return (
        <div className='flex flex-row items-center w-4 mr-3'>
            {dir
                ? <FaFolder />
                : name.endsWith('.mp4') || name.endsWith('.mkv') || name.endsWith('.avi') || name.endsWith('.mov')
                ? <FaFilm />
                : name.endsWith('.pdf') || name.endsWith('.doc') || name.endsWith('.docx') || name.endsWith('.txt')
                ? <FaFilePdf />
                : name.endsWith('.jpg') || name.endsWith('.jpeg') || name.endsWith('.png') || name.endsWith('.gif')
                ? <FaImage />
                : name.endsWith('.mp3') || name.endsWith('.wav') || name.endsWith('.flac') || name.endsWith('.ogg')
                ? <FaMusic />
                : name.endsWith('.zip') || name.endsWith('.tar') || name.endsWith('.gz') || name.endsWith('.rar') || name.endsWith('.7z') || name.endsWith('.tgz') || name.endsWith('.xz') || name.endsWith('.bz2')
                ? <FaFileZipper />
                : name.endsWith('.js') || name.endsWith('.json')
                ? <FaJs />
                : name.endsWith('.html') || name.endsWith('.css') || name.endsWith('.scss') || name.endsWith('.less') || name.endsWith('.sass')
                ? <FaCode />
                : name.endsWith('.iso')
                ? <FaBoxArchive />
                : <FaFile />
            }
        </div>
    );
}