// QA opcional: requiere Playwright del entorno de pruebas, sin dependencias nuevas del producto.
const {chromium}=require('playwright');
const http=require('http'),fs=require('fs'),path=require('path');
(async()=>{
 const root=path.resolve(process.argv[2]||'');
 if(process.argv.length!==3)throw Error('Uso: node scripts/check_thumbnail_browser.cjs DIRECTORIO_DE_LA_SONDA');
 const report=JSON.parse(fs.readFileSync(path.join(root,'rendering.json'),'utf8'));
 if(!['source.jpg','source.png'].includes(report.input))throw Error('Formato de entrada no permitido');
 const html=`<!doctype html><html lang="es"><meta charset="utf-8"><title>Comparación de miniaturas</title><style>body{margin:24px;background:#f5f5f5;color:#222;font:16px system-ui}section{display:flex;gap:24px}figure{margin:0}img{display:block;width:480px;height:270px}figcaption{margin:10px 0}small{display:block}#decoded{display:none}</style><h1>Mismo archivo, distinta salida</h1><p>${report.synthetic?'Patrón sintético. Filas: detalle de 1, 6 y 12 píxeles. Sin fotos ni cuentas.':'Archivo de prueba proporcionado.'}</p><section><figure><img id="browser" src="${report.input}"><figcaption>Imagen real en navegador</figcaption></figure><figure><img id="terminal" src="halfblocks-enlarged.png"><figcaption>Renderizador actual de cbrec · 60×20 celdas</figcaption></figure></section><img id="decoded" src="decoded.png"></html>`;
 const server=http.createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html; charset=utf-8');res.end(html);return;}
  const name=req.url.slice(1);
  if(![report.input,'decoded.png','halfblocks-enlarged.png'].includes(name)){res.writeHead(404).end();return;}
  res.setHeader('Content-Type',name.endsWith('.jpg')?'image/jpeg':'image/png');res.end(fs.readFileSync(path.join(root,name)));
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve)); let browser;
 try{
  browser=await chromium.launch({headless:true,...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE?{executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE}:{})});
  const page=await browser.newPage({viewport:{width:1050,height:460},deviceScaleFactor:1});
  await page.goto(`http://127.0.0.1:${server.address().port}/`,{waitUntil:'networkidle'});
  const result=await page.evaluate(()=>{
   const capture=(id,w,h)=>{const canvas=new OffscreenCanvas(w,h),ctx=canvas.getContext('2d');ctx.drawImage(document.getElementById(id),0,0,w,h);return ctx.getImageData(0,0,w,h).data;};
   const mae=(a,b)=>{let total=0,count=0;for(let i=0;i<a.length;i++)if(i%4!==3){total+=Math.abs(a[i]-b[i]);count++;}return total/count;};
   const contrast=(a,w,y)=>{let values=[];for(let x=0;x<w;x++)values.push(a[(y*w+x)*4]);const mean=values.reduce((a,b)=>a+b,0)/w;return Math.sqrt(values.reduce((a,b)=>a+(b-mean)**2,0)/w);};
   const reference=document.getElementById('browser'),width=reference.naturalWidth,height=reference.naturalHeight;
   const a=capture('browser',width,height),decoded=capture('decoded',width,height),low=capture('terminal',width,height);
   const aw=capture('browser',179,101),bw=capture('terminal',179,101);
   return {source:[width,height],decoder_mae_rgb:mae(a,decoded),fallback_mae_rgb:mae(a,low),web_card:[179,101],stripe_contrast_browser:contrast(aw,179,50),stripe_contrast_fallback:contrast(bw,179,50),images:Array.from(document.images).map(img=>({id:img.id,natural:[img.naturalWidth,img.naturalHeight]}))};
  });
  if(report.synthetic&&result.decoder_mae_rgb>=2)throw Error('La decodificación difiere del navegador más de lo esperado: '+JSON.stringify(result));
  if(report.synthetic&&result.stripe_contrast_browser<=result.stripe_contrast_fallback*2)throw Error('La prueba no distingue suficiente pérdida de detalle');
  await page.screenshot({path:path.join(root,'comparison.png')});
  fs.writeFileSync(path.join(root,'browser-comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
 }finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
