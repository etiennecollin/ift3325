var searchIndex = new Map(JSON.parse('[["client",{"t":"HHH","n":["main","send_file","setup_connection"],"q":[[0,"client"],[3,"alloc::vec"],[4,"tokio::sync::mpsc::bounded"],[5,"utils::window"],[6,"alloc::string"],[7,"core::result"],[8,"tokio::net::tcp::stream"]],"i":"```","f":"{{}b}{{{h{{f{d}}}}jln}{{Ad{{Ab{A`}}{Ab{A`}}}}}}{{Afdn}b}","D":"h","p":[[1,"unit"],[1,"u8"],[5,"Vec",3],[5,"Sender",4],[8,"SafeWindow",5],[8,"SafeCond",5],[5,"String",6],[1,"str"],[1,"reference"],[6,"Result",7],[5,"TcpStream",8]],"r":[],"b":[],"c":"OjAAAAAAAAA=","e":"OjAAAAAAAAA="}],["server",{"t":"SHHH","n":["OUTPUT_DIR","assembler","handle_client","main"],"q":[[0,"server"],[4,"alloc::vec"],[5,"tokio::sync::mpsc::bounded"],[6,"core::net::socket_addr"],[7,"core::result"],[8,"tokio::net::tcp::stream"]],"i":"````","f":"`{{{f{{d{b}}}}h}{{n{{l{j}}{l{j}}}}}}{{A`h}{{n{Ab{l{j}}}}}}{{}Ad}","D":"f","p":[[1,"u8"],[5,"Vec",4],[5,"Receiver",5],[6,"SocketAddr",6],[1,"str"],[1,"reference"],[6,"Result",7],[5,"TcpStream",8],[1,"bool"],[1,"unit"]],"r":[],"b":[],"c":"OjAAAAAAAAA=","e":"OjAAAAEAAAAAAAEAEAAAAAEAAgA="}],["tunnel",{"t":"HHHHH","n":["handle_client","handle_connection","handle_server","main","writer"],"q":[[0,"tunnel"],[5,"tokio::net::tcp::split_owned"],[6,"alloc::vec"],[7,"tokio::sync::mpsc::bounded"],[8,"core::result"],[9,"tokio::runtime::task::join"],[10,"tokio::net::tcp::stream"]],"i":"`````","f":"{{b{h{{f{d}}}}jj}{{Ad{{Ab{l{A`{n}}}}}}}}{{AfAfjj}l}1{{}l}{{Ah{Aj{{f{d}}}}}{{Ad{{Ab{l{A`{n}}}}}}}}","D":"b","p":[[5,"OwnedReadHalf",5],[1,"u8"],[5,"Vec",6],[5,"Sender",7],[1,"f32"],[1,"unit"],[1,"str"],[1,"reference"],[6,"Result",8],[5,"JoinHandle",9],[5,"TcpStream",10],[5,"OwnedWriteHalf",5],[5,"Receiver",7]],"r":[],"b":[],"c":"OjAAAAAAAAA=","e":"OjAAAAEAAAAAAAQAEAAAAAAAAQACAAMABQA="}],["utils",{"t":"CCCCCHHSSSSSSIHHPTPPPTFGGPPPPTTPPTPPPNNNNNNOOOONNONNNNNNNNNNONNNNNNNNNNHHHHHHTPTTTTIIFGNNNNNNNONNNNNONNNNNNOONNNNNNO","n":["byte_stuffing","crc","frame","io","window","byte_destuffing","byte_stuffing","FINAL_XOR","INITIAL_VALUE","LUT","MSB","POLYNOMIAL","POLYNOMIAL_WIDTH","PolynomialSize","crc_16_ccitt","lookup_table","AbortSequenceReceived","BOUNDARY_FLAG","ConnectionStart","ConnexionEnd","DestuffingError","ESCAPE_FLAG","Frame","FrameError","FrameType","Information","InvalidFCS","InvalidFrameType","InvalidLength","MAX_SIZE","MAX_SIZE_DATA","MissingBoundaryFlag","P","REPLACEMENT_POSITION","ReceiveReady","Reject","Unknown","borrow","","","borrow_mut","","","content","content_stuffed","data","fcs","fmt","","frame_type","from","","","","from_bytes","generate_content","into","","","new","num","to_bytes","try_from","","","try_into","","","type_id","","","connection_request","create_frame_timer","flatten","handle_reception","reader","writer","FRAME_TIMEOUT","Full","MAX_FRAME_NUM","NUMBERING_BITS","SIZE_GO_BACK_N","SIZE_SREJ","SafeCond","SafeWindow","Window","WindowError","borrow","","borrow_mut","","contains","default","fmt","frames","from","","get_size","into","","is_connected","is_empty","is_full","new","pop_front","pop_until","push","resend_all","srej","try_from","","try_into","","type_id","","waiting_disconnect"],"q":[[0,"utils"],[5,"utils::byte_stuffing"],[7,"utils::crc"],[16,"utils::frame"],[71,"utils::io"],[77,"utils::window"],[116,"alloc::vec"],[117,"core::result"],[118,"core::fmt"],[119,"core::any"],[120,"core::option"],[121,"tokio::sync::mpsc::bounded"],[122,"tokio::runtime::task::join"],[123,"tokio::net::tcp::split_owned"]],"i":"````````````````jAhAj021```0222112010000210211111211002111021111021021021``````CdCf1111````10101101101101111111111010101","f":"`````{{{f{{d{b}}}}}{{l{{h{b}}j}}}}{{{f{{d{b}}}}}{{h{b}}}}```````{{{f{{d{b}}}}}n}{{}{{A`{n}}}}`````````````````````{f{{f{c}}}{}}00{{{f{Ab}}}{{f{Abc}}}{}}00````{{{f{j}}{f{AbAd}}}Af}{{{f{Ah}}{f{AbAd}}}Af}`{cc{}}{bAj}11{{{f{{d{b}}}}}{{l{Ahj}}}}{{{f{AbAh}}}Al}{{}c{}}00{{Ajb{h{b}}}Ah}`{{{f{Ah}}}{{h{b}}}}{c{{l{e}}}{}{}}00{{}{{l{c}}}{}}00{fAn}00{{{f{B`}}Bb{Bd{b}}{Bf{{h{b}}}}{f{Bh}}}Al}{{B`b{Bf{{h{b}}}}}Al}{{{Bl{{l{c{f{Bj}}}}}}}{{l{c{f{Bj}}}}}{}}{{Ah{f{B`}}{f{Bh}}{Bd{{f{{Bf{{h{b}}}}}}}}{Bd{{f{{Bf{{h{b}}}}}}}}{f{Abb}}}Bb}{{BnB`Bh{Bd{{Bf{{h{b}}}}}}{Bd{{Bf{{h{b}}}}}}}{{Bl{{l{{f{Bj}}{f{Bj}}}}}}}}{{C`{Cb{{h{b}}}}}{{Bl{{l{{f{Bj}}{f{Bj}}}}}}}}``````````{f{{f{c}}}{}}0{{{f{Ab}}}{{f{Abc}}}{}}0{{{f{Cd}}b}Bb}{{}Cd}{{{f{Cf}}{f{AbAd}}}Af}`{cc{}}0{{{f{Cd}}}Ch}{{}c{}}0`{{{f{Cd}}}Bb}05{{{f{AbCd}}{f{Bh}}}{{Bd{Ah}}}}{{{f{AbCd}}bBb{f{Bh}}}Ch}{{{f{AbCd}}Ah}{{l{AlCf}}}}``{c{{l{e}}}{}{}}0{{}{{l{c}}}{}}0{fAn}0`","D":"Gh","p":[[1,"u8"],[1,"slice"],[1,"reference"],[5,"Vec",116],[6,"FrameError",16],[6,"Result",117],[1,"u16"],[1,"array"],[0,"mut"],[5,"Formatter",118],[8,"Result",118],[5,"Frame",16],[6,"FrameType",16],[1,"unit"],[5,"TypeId",119],[8,"SafeWindow",77],[1,"bool"],[6,"Option",120],[5,"Sender",121],[8,"SafeCond",77],[1,"str"],[5,"JoinHandle",122],[5,"OwnedReadHalf",123],[5,"OwnedWriteHalf",123],[5,"Receiver",121],[5,"Window",77],[6,"WindowError",77],[1,"usize"]],"r":[],"b":[],"c":"OjAAAAAAAAA=","e":"OzAAAAEAADgADwAAAAUACAAGABEAAAAVAAAAGwACACAAAAAmAAUAMAABADQAAAA/AAgAVAABAFcABABdAAIAZQAAAGwACAA="}]]'));
if (typeof exports !== 'undefined') exports.searchIndex = searchIndex;
else if (window.initSearch) window.initSearch(searchIndex);
//{"start":39,"fragment_lengths":[520,527,690,3300]}