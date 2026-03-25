const server = require("NeteaseCloudMusicApi/server");

const PORT = 10754;

server
  .serveNcmApi({
    port: PORT,
    checkVersion: false,
  })
  .then(() => {
    console.log(`API 服务已在本地端口 ${PORT} 就绪`);
  })
  .catch((err) => {
    console.error("启动失败:", err);
    process.exit(1);
  });
