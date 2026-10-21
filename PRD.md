Versión 0.5 (agrega procesamiento por canal de entrada, presets de canal, limitador en el máster de cada mezcla, reverb y delay compartidos con envíos por canal y retornos por mezcla, y medidor de carga del motor; la 0.4 agregó historial de salud del enlace, buffer del celular desde la consola, redundancia de paquetes, archivo de registro, fuente de demostración y respaldo automático de mezclas; la 0.3 agregó mezcla local, grabación multipista y soundcheck virtual), ajustada para una primera validación técnica realizada por una sola persona, con 1 a 3 horas de trabajo por noche.

# PRD: Sistema de monitoreo personal inalámbrico (nombre provisional: _BandEar_)

## 1. Resumen

Un sistema de software que toma los canales de una mezcladora digital o interfaz de audio conectada a una computadora y entrega a cada músico, en su celular, una mezcla personal que él mismo controla. Está pensado para grupos de 10 o más músicos y para estudios, con apps nativas en iOS y Android.

## 2. Objetivos

- Que cada músico controle su mezcla (volumen, paneo y mute por canal) desde el celular.
- Latencia total de audio baja y estable, sin chasquidos.
- Soportar 10 a 16 músicos simultáneos desde un solo servidor.
- Configuración simple: sin pedir la IP a mano ni instalar drivers en el celular.

## 3. Fuera de alcance (v1)

- Procesamiento por músico (un EQ o un compresor distinto en la mezcla de cada uno) y procesamiento desde el celular: en v1 el procesamiento es por canal de entrada, compartido por todas las mezclas, y se edita solo desde la consola (RF21 a RF25).
- Control de la mezcla principal de la consola.
- Edición o recorte de tomas grabadas, y remuestreo de la salida local (la grabación multipista y el soundcheck virtual sí entran en v1: RF13 a RF16).
- Transmisión por internet; v1 funciona solo en red local.
- Escalado a 10-16 músicos antes de validar el flujo con 1-2 dispositivos.

## 4. Usuarios

- **Músico:** quiere escucharse bien y ajustar su mezcla sin molestar al ingeniero.
- **Ingeniero o técnico:** configura canales, nombres, grupos, límites y mezclas base, y supervisa la conexión de todos.
- **Administrador del estudio:** gestiona salas, presets e historial de sesiones.

## 5. Requerimientos funcionales

**Servidor (Rust + Tauri)**

- RF1. Seleccionar dispositivo de audio, frecuencia de muestreo y tamaño de buffer.
- RF2. Capturar hasta 32 canales de entrada.
- RF3. Generar una mezcla estéreo independiente por músico.
- RF4. Enviar el audio por UDP a cada cliente y recibir su control por un canal aparte.
- RF5. Mostrar medidores, estado de conexión y pérdida de paquetes por cliente, con un historial por músico de muestras de un segundo (los últimos 10 minutos) en un gráfico: pérdida, tiempo de ida y vuelta del canal de control, y el jitter y el colchón que informa el celular, con los segundos sin informe, las reconexiones, los cortes y los tiempos de espera marcados.
- RF6. Descubrimiento automático (mDNS) y código QR para conectar.
- RF7. Guardar y cargar presets de sesión y de mezcla.
- RF12. Permitir al ingeniero bloquear canales y establecer límites de volumen por músico.
- RF13. Mezcla local: una mezcla propia del ingeniero, editable como cualquier otra, que suena en una salida de la computadora (ninguna por defecto). Con ASIO solo se ofrecen las salidas del mismo driver; con WASAPI, cualquier salida, compensando la deriva de reloj con relleno o descarte de muestras.
- RF14. Grabación multipista mientras suena el show: cada entrada armada a su propio archivo WAV (un par enlazado en estéreo, y la mezcla local si se pide), en 24 bits enteros o 32 bits flotante a 48 kHz, RF64 por encima de 4 GB y cabeceras refrescadas para que un corte deje archivos legibles. Una carpeta por toma con su `take.json` (pistas, inicio, marcadores, muestras perdidas), marcadores durante la toma y estimación del tiempo de disco restante. No se cambia de dispositivo mientras se graba.
- RF15. Soundcheck virtual: reproducir una toma dentro del mezclador en lugar de las entradas en vivo, con pausa, búsqueda, bucle y vuelta inmediata a las entradas. Solo tomas de 48 kHz cuyas pistas encajen en las entradas y pares actuales; mientras suena no se graba ni se cambia de dispositivo.
- RF16. Pantalla de grabación en la app de escritorio: transporte, una pista por entrada con armado, medidor y forma de onda, lista de tomas, ajustes (carpeta, formato, mezcla local) y la tarjeta del soundcheck virtual.
- RF17. Redundancia de paquetes por músico: cada paquete de audio se envía dos veces, duplicando el ancho de banda solo de ese músico. Por defecto la copia sale justo después del original (sin latencia añadida, la acepta cualquier celular, cubre pérdidas aisladas); opcionalmente sale 1 a 4 bloques después, para ráfagas de pérdida en el WiFi, solo para un celular que retiene los paquetes ese tiempo, a costa de esa latencia (unos 5,3 ms por bloque de 256 muestras a 48 kHz). El celular informa los paquetes recuperados por una copia diferida.
- RF18. Archivo de registro: el motor y la app de escritorio escriben sus eventos en `bandear.log`, en la carpeta de registros de la app, con hora UTC, nivel y área; rota a los 5 MB y conserva 5 archivos anteriores. Una versión sin consola sigue dejando diagnóstico.
- RF19. Fuente de demostración: una señal generada (un loop de 8 canales a -18 dBFS, igual en cada ejecución) elegible como driver "Demo (no interface)", para correr el motor y transmitir a los celulares sin interfaz de audio. Nunca se elige sola; permite grabar, pero no el soundcheck virtual.
- RF20. Respaldo automático de las mezclas de los músicos: unos segundos después de que dejan de cambiar (y al detener el motor o cerrar la app) se guardan en un respaldo rotativo de 10 copias, escrito de forma atómica. Restaurar es decisión del ingeniero desde el panel de sesión y solo alcanza a los músicos conectados; la mezcla propia de un celular que se reconecta no se reemplaza sola.
- RF21. Procesamiento por canal de entrada, como en una consola: filtro pasa altos, ecualizador paramétrico de 4 bandas (shelf de graves, dos picos y shelf de agudos), puerta de ruido y compresor, en ese orden (pasa altos, puerta, ecualizador, compresor). Un par estéreo enlazado es un solo canal con un detector compartido. Todas las mezclas oyen el canal procesado (los celulares, la salida local y la mezcla local grabada); las pistas grabadas de cada entrada quedan como se capturaron. Se edita solo desde la consola, con la curva de respuesta del ecualizador, medidores de entrada y de salida procesada, medidores de reducción de ganancia de la puerta y del compresor, y comparación A/B. Un canal con todo apagado deja las muestras idénticas bit a bit. Se guarda en la sesión y en las sesiones con nombre.
- RF22. Presets de canal: cinco de fábrica ("Vocal", "Kick", "Bass DI", "Acoustic guitar" y "Gentle compression"), que no se borran, y los del ingeniero con cualquier otro nombre. Aplicar un preset, copiar o reiniciar un canal no cambia sus envíos a los efectos.
- RF23. Limitador de picos en el máster de cada mezcla (la de cada músico y la mezcla local), después de la suma y de los retornos de efectos y sin lookahead: techo de -24 a 0 dBFS (rige el menor entre el suyo y el de la mezcla) y release de 1 a 1000 ms, con medidor de reducción de ganancia. Solo lo maneja el ingeniero; apagado por defecto, la mezcla queda exactamente como sin él.
- RF24. Efectos compartidos: una reverb (tamaño, amortiguación, pre-delay y amplitud estéreo) y un delay (tiempo de 1 a 2000 ms o una nota a un tempo, realimentación, corte de agudos en las repeticiones y ping-pong). Cada entrada tiene un envío a cada efecto, tomado después de su procesamiento y antes de los faders, y cada mezcla un retorno de cada efecto que solo maneja el ingeniero. Se guardan en la sesión.
- RF25. Medidor de carga del motor: el tiempo que tarda el motor en procesar cada bloque, como porcentaje del período del bloque (promedio de alrededor de un segundo, pico de los últimos segundos y bloques tardíos), con aviso por encima del 70 %.

**Apps móviles (Swift y Kotlin)**

- RF8. Conectarse al servidor, elegir su perfil de músico y recibir el audio.
- RF9. Faders, paneo, mute y solo por canal, con grupos (voces, batería, etc.).
- RF10. Mezclas guardadas (bancos) y botón de "más de mí".
- RF11. Indicador de estabilidad y ajuste del buffer. El ingeniero elige por músico, desde la consola, "Auto" (el colchón adaptativo del celular) o un buffer fijo de 5 a 500 ms que el celular aplica sin adaptarlo; la consola muestra lo que el celular reproduce realmente. El celular informa su jitter, su colchón y el audio en cola.

## 6. Requerimientos no funcionales

- **Latencia de extremo a extremo:** objetivo ≤ 25 ms, aceptable para MVP ≤ 40 ms. El procesamiento agrega 0 ms: los filtros son IIR y la puerta, el compresor y el limitador trabajan sin lookahead; la reverb y el delay son retornos (señal húmeda) que no retrasan la señal directa.
- **Carga del motor:** en una compilación release, 32 canales procesados con 16 músicos usan en promedio el 17,6 % del período de cada bloque (25,9 % en el percentil 99), sin bloques tardíos.
- **Estabilidad:** sin cortes audibles en sesiones de 3 horas.
- **Ancho de banda aproximado** (48 kHz, estéreo, por músico): PCM 16 bits ≈ 1,5 Mbps; PCM 24 bits ≈ 2,3 Mbps; Opus ≈ 0,1 a 0,2 Mbps. Con 16 músicos, el PCM suma unos 25 a 37 Mbps. Con Opus, solo unos 2 a 3 Mbps.
- **Plataformas:** servidor en Windows y macOS (Linux opcional); iOS 16+ y Android 10+.
- **Privacidad:** sin recopilación de datos; todo funciona en red local.

## 7. Decisiones técnicas

- **Transporte:** UDP unicast con números de secuencia y buffer anticipado adaptativo, o fijo si el ingeniero lo pide. La redundancia reenvía el mismo datagrama sin cambiar su formato; las copias diferidas se guardan en memoria reservada fuera del hilo de audio, que nunca reserva memoria ni bloquea.
- **Compatibilidad:** el celular anuncia en el registro una lista opcional de capacidades (estadísticas ampliadas, buffer desde la consola, retención para copias diferidas, paquetes recuperados). El servidor solo envía los mensajes nuevos a quien los anunció y la consola muestra "needs a newer app" en los demás; los celulares y servidores anteriores siguen funcionando, porque los campos desconocidos se ignoran.
- **Tiempo de ida y vuelta:** se mide con el ping y pong del canal de control cada 2 s, suavizado como el SRTT de TCP, en cualquier celular.
- **Códec:** PCM en v1; Opus con tramas de 2,5 a 5 ms como opción para redes saturadas.
- **Audio interno:** flotante de 32 bits a 48 kHz desde la captura; la conversión a 16 bits ocurre solo al enviar a los celulares, así que la grabación conserva los 24 bits de la entrada. Las entradas a 96 o 192 kHz pasan por un filtro antialias antes de diezmar.
- **Procesamiento:** una etapa en el hilo del motor, entre la captura y las mezclas. Los coeficientes se calculan fuera del hilo de audio y llegan por una cola sin bloqueos; el hilo de audio nunca reserva memoria ni bloquea. Los cambios de ajustes y de ganancia se deslizan para no producir chasquidos, y los efectos reservan sus líneas de retardo una sola vez.
- **Grabación:** escritor WAV/RF64 propio; el audio va a disco en un hilo aparte por un búfer sin bloqueos, y si el búfer se llena se descartan y cuentan muestras sin bloquear nunca el audio.
- **Audio móvil:** AVAudioEngine en iOS y Oboe/AAudio en Android.
- **Control:** WebSocket para faders, nombres y estado.
- **Red:** el cuello de botella con 10+ clientes es el WiFi, no la CPU. Se recomienda un router o AP dedicado en 5 GHz, y varios puntos de acceso para grupos grandes. La app debe avisar de redes inadecuadas.

## 8. Hitos

1. **M0 – Prueba de concepto:** captura desde la Volt 4 en Windows, mezcla estéreo básica, UDP a un iPhone o Android y medición de latencia y pérdida de paquetes.
2. **M1 – Mezclador y control:** mezclas por músico, canal de control, 4 clientes.
3. **M2 – Apps nativas:** interfaz completa en iOS y Android.
4. **M3 – Escala:** 10 a 16 clientes, Opus, métricas de estabilidad.
5. **M4 – Producto:** presets, instalador, documentación y beta con usuarios reales.

## 8.1 Plan de trabajo para M0

El objetivo de M0 es demostrar que el recorrido completo funciona antes de invertir en una interfaz móvil completa o en soporte multicanal.

1. **Preparar el entorno:** crear el proyecto Rust, seleccionar Windows como plataforma principal y comprobar que la Volt 4 aparece como dispositivo de entrada.
2. **Capturar audio:** leer audio PCM a 48 kHz, con tamaño de buffer configurable y registro de errores del dispositivo.
3. **Enviar audio:** empaquetar audio estéreo con números de secuencia y transmitirlo por UDP dentro de la red local.
4. **Reproducir en un cliente:** implementar un cliente de prueba para iPhone y Android, priorizando primero la plataforma que permita medir antes.
5. **Medir:** registrar latencia extremo a extremo, pérdida de paquetes, jitter y cortes audibles durante pruebas repetibles.
6. **Control mínimo:** agregar un canal de control para volumen, mute y límite de volumen del músico.
7. **Validar en macOS:** repetir la prueba básica con la Volt 4 y documentar diferencias respecto de Windows.
8. **Preparar el siguiente escenario:** probar la X32 con X-USB solo después de que M0 sea estable con la Volt 4.

### Criterios de aceptación de M0

- Un teléfono recibe y reproduce audio de la Volt 4 dentro de la misma red local.
- La latencia medida queda documentada, con objetivo de 25 ms o menos y límite de aceptación de 40 ms.
- La prueba informa pérdida de paquetes y cortes, en lugar de ocultarlos con reconexiones silenciosas.
- El ingeniero puede silenciar un canal y limitar el volumen máximo del músico.
- La prueba se puede repetir en Windows y queda preparada para una validación posterior en macOS.

## 9. Métricas de éxito

- Latencia medida de extremo a extremo.
- Pérdida de paquetes por cliente por debajo del 0,5 %.
- Tiempo de conexión de un músico nuevo por debajo de 30 s.
- Sesiones de 3 horas sin interrupciones con 10 clientes.

## 10. Riesgos

- Latencia o chasquidos por WiFi saturado (el riesgo más grande).
- Fragmentación de dispositivos Android, con latencias de audio muy distintas.
- Soporte de ASIO en Windows para interfaces multicanal.
- Alcance excesivo para un solo desarrollador; conviene validar primero con 1 o 2 clientes.
- Marca y diseño: crear una identidad propia, sin reutilizar nombre ni elementos de StageWave.

## 11. Preguntas abiertas

1. ¿El producto será de licencia única, suscripción o código abierto con versión de pago?
2. ¿Qué alcance tendrá la gestión de salas, presets e historial para la primera versión comercial?

Estas decisiones quedan fuera de M0 y se definirán antes de diseñar la primera versión comercial.
